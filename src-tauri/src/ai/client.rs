use super::gateway::{self, EmbeddingIdentity};
use super::settings::{AiProvider, AiSettings};
use crate::error::{AppError, AppResult};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system",
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user",
            content: content.into(),
        }
    }
}

pub trait LlmClient: Send + Sync {
    /// Streams the reply through `on_delta`; returning `false` from it cancels the request.
    /// `on_delta` is also called with an empty string while the model produces hidden
    /// reasoning, so cancellation still works then.
    fn chat(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String>;

    /// Like `chat`, but asks the service to stop after `max_tokens` generated tokens,
    /// reasoning included, so a runaway reply cannot stall batch work.
    fn chat_limited(
        &self,
        messages: &[ChatMessage],
        max_tokens: u32,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        let _ = max_tokens;
        self.chat(messages, on_delta)
    }

    /// Returns one L2-normalized vector per input, in input order.
    fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>>;

    fn embed_model(&self) -> &str;

    fn validate_embed_model(&self) -> AppResult<()> {
        Ok(())
    }
}

pub fn cancelled() -> AppError {
    AppError::Ai("已停止生成".into())
}

pub struct OpenAiCompatClient {
    agent: ureq::Agent,
    base_url: String,
    api_key: Option<String>,
    chat_model: String,
    embed_model: String,
    provider: AiProvider,
    allow_cloud: bool,
    cloud_consent: bool,
    index_model: String,
    approved_identity: Option<EmbeddingIdentity>,
}

impl OpenAiCompatClient {
    pub fn new(settings: &AiSettings, api_key: Option<String>) -> Self {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(300))
            .timeout_write(Duration::from_secs(60));
        if settings.provider == AiProvider::Xcli {
            builder = builder.redirects(0).try_proxy_from_env(false);
        }
        Self {
            agent: builder.build(),
            base_url: settings.base_url.trim_end_matches('/').to_string(),
            api_key,
            chat_model: settings.chat_model.clone(),
            embed_model: settings.embed_model.clone(),
            provider: settings.provider,
            allow_cloud: settings.allow_cloud,
            cloud_consent: false,
            index_model: if settings.provider == AiProvider::Xcli {
                format!("{}|xcli:unapproved", settings.embed_model)
            } else {
                settings.embed_model.clone()
            },
            approved_identity: None,
        }
    }

    pub fn with_cloud_consent(mut self, consent: bool) -> Self {
        self.cloud_consent = consent;
        self
    }

    pub fn with_index_identity(mut self, identity: Option<EmbeddingIdentity>) -> Self {
        if self.provider == AiProvider::Xcli {
            if let Some(identity) = identity.filter(|identity| {
                gateway::endpoint(&self.base_url).ok().as_ref() == Some(&identity.endpoint)
                    && identity.alias == self.embed_model
            }) {
                self.index_model = identity.key();
                self.approved_identity = Some(identity);
            }
        }
        self
    }

    fn gateway_catalog(&self) -> AppResult<Value> {
        gateway::endpoint(&self.base_url)?;
        if self
            .api_key
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(AppError::Ai(
                "请填写 xcli 为 Chat Insight 签发的独立应用 Token".into(),
            ));
        }
        let response = self
            .request("GET", "/models")
            .call()
            .map_err(|error| self.map_error(error))?;
        let mut body = String::new();
        response
            .into_reader()
            .take(4 * 1024 * 1024 + 1)
            .read_to_string(&mut body)?;
        if body.len() > 4 * 1024 * 1024 {
            return Err(AppError::Ai("xcli 模型目录超过大小限制".into()));
        }
        serde_json::from_str(&body)
            .map_err(|_| AppError::Ai("xcli 模型目录无效；未发送聊天内容".into()))
    }

    fn gateway_model(&self, alias: &str, capability: &str) -> AppResult<Value> {
        let catalog = self.gateway_catalog()?;
        let selected = gateway::selected(&catalog, alias, capability)?;
        gateway::ensure_consent(selected, self.allow_cloud, self.cloud_consent)?;
        Ok(selected.clone())
    }

    fn current_embedding_identity(&self) -> AppResult<EmbeddingIdentity> {
        let metadata = self.gateway_model(&self.embed_model, "embeddings")?;
        EmbeddingIdentity::from_model(&self.base_url, &self.embed_model, &metadata)
    }

    pub fn prepare_index(&mut self, rebuild: bool) -> AppResult<Option<EmbeddingIdentity>> {
        if self.provider != AiProvider::Xcli {
            return Ok(None);
        }
        let identity = self.current_embedding_identity()?;
        if !rebuild && self.approved_identity.as_ref() != Some(&identity) {
            return Err(gateway::rebuild_required());
        }
        self.index_model = identity.key();
        self.approved_identity = Some(identity.clone());
        Ok(Some(identity))
    }

    /// This is only the fixed, non-private connection-test text, never a chat input.
    pub fn test_embedding_dimension(&self) -> AppResult<usize> {
        Ok(self
            .embeddings(&["连接测试".into()], false)?
            .first()
            .map(Vec::len)
            .unwrap_or(0))
    }

    pub fn gateway_privacy(&self) -> AppResult<Option<GatewayPrivacy>> {
        if self.provider != AiProvider::Xcli {
            return Ok(None);
        }
        let catalog = self.gateway_catalog()?;
        let chat = gateway::selected(&catalog, &self.chat_model, "text.generate")?;
        let embed = gateway::selected(&catalog, &self.embed_model, "embeddings")?;
        let location = |model: &Value| {
            match model
                .pointer("/xcli/inference_location")
                .and_then(Value::as_str)
            {
                Some("local") => "local",
                Some("remote") => "remote",
                _ => "unknown",
            }
            .to_string()
        };
        Ok(Some(GatewayPrivacy {
            chat_cloud: gateway::effective_cloud(chat),
            embedding_cloud: gateway::effective_cloud(embed),
            chat_location: location(chat),
            embedding_location: location(embed),
        }))
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let mut request = self
            .agent
            .request(method, &format!("{}{path}", self.base_url))
            .set("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        request
    }

    fn post(&self, path: &str, body: &Value) -> AppResult<ureq::Response> {
        let mut request = self.request("POST", path);
        if self.provider == AiProvider::Xcli {
            let alias = body
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::Ai("缺少 xcli 模型名称".into()))?;
            let capability = match path {
                "/embeddings" => "embeddings",
                "/chat/completions" => "text.generate",
                _ => return Err(AppError::Ai("不支持的 xcli 请求".into())),
            };
            let model = self.gateway_model(alias, capability)?;
            if body.get("stream").and_then(Value::as_bool) == Some(true) {
                let catalog = serde_json::json!({"object":"list","data":[model.clone()]});
                gateway::selected(&catalog, alias, "text.stream")?;
            }
            if gateway::effective_cloud(&model) && self.cloud_consent {
                request = request.set("X-Xcli-Remote-Consent", "allow");
            }
        }
        request
            .send_string(&body.to_string())
            .map_err(|error| self.map_error(error))
    }

    fn map_error(&self, error: ureq::Error) -> AppError {
        if self.provider == AiProvider::Xcli {
            return AppError::Ai(match error {
                ureq::Error::Status(code, _) => format!(
                    "xcli 请求失败（HTTP {code}），请检查服务、应用 Token、模型能力及本次云端授权"
                ),
                _ => "无法连接 xcli 本机网关，请检查服务状态".into(),
            });
        }
        match error {
            ureq::Error::Status(code, response) => {
                let detail = response
                    .into_string()
                    .ok()
                    .and_then(|body| {
                        serde_json::from_str::<Value>(&body).ok().and_then(|value| {
                            value
                                .pointer("/error/message")
                                .or_else(|| value.get("error"))
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                    })
                    .unwrap_or_default();
                let hint = match code {
                    401 | 403 => "，请检查 API Key",
                    404 => "，请确认模型名称和服务地址（通常以 /v1 结尾）",
                    _ => "",
                };
                let detail: String = detail.chars().take(200).collect();
                AppError::Ai(format!(
                    "模型服务返回错误（{code}）{hint}{}",
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!("：{detail}")
                    }
                ))
            }
            ureq::Error::Transport(_) => AppError::Ai(format!(
                "无法连接模型服务 {}。如使用 Ollama，请确认它已启动并已下载所需模型",
                self.base_url
            )),
        }
    }

    pub fn list_models(&self) -> AppResult<Vec<String>> {
        if self.provider == AiProvider::Xcli {
            let catalog = self.gateway_catalog()?;
            let models = catalog
                .get("data")
                .and_then(Value::as_array)
                .filter(|_| catalog.get("object").and_then(Value::as_str) == Some("list"))
                .ok_or_else(|| AppError::Ai("xcli 模型目录无效".into()))?;
            return Ok(models
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect());
        }
        let body = self
            .request("GET", "/models")
            .call()
            .map_err(|error| self.map_error(error))?
            .into_string()?;
        let value: Value = serde_json::from_str(&body)?;
        Ok(value
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPrivacy {
    pub chat_cloud: bool,
    pub embedding_cloud: bool,
    pub chat_location: String,
    pub embedding_location: String,
}

impl OpenAiCompatClient {
    fn complete(
        &self,
        messages: &[ChatMessage],
        max_tokens: Option<u32>,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        let mut body = json!({
            "model": self.chat_model,
            "messages": messages,
            "stream": true,
            "temperature": 0.3,
        });
        if let Some(limit) = max_tokens {
            body["max_tokens"] = json!(limit);
        }
        let response = self.post("/chat/completions", &body)?;
        let is_event_stream = response
            .header("Content-Type")
            .is_some_and(|value| value.contains("text/event-stream"));
        let mut filter = ThinkFilter::default();
        let mut output = String::new();
        if !is_event_stream {
            if self.provider == AiProvider::Xcli {
                return Err(AppError::Ai(
                    "xcli 未返回流式响应，未降级为一次性成功结果".into(),
                ));
            }
            let mut body = String::new();
            response.into_reader().read_to_string(&mut body)?;
            let value: Value = serde_json::from_str(&body)?;
            let content = value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let visible = filter.push(content) + &filter.finish();
            output.push_str(&visible);
            if !visible.is_empty() && !on_delta(&visible) {
                return Err(cancelled());
            }
            return Ok(output.trim().to_string());
        }
        let reader = BufReader::new(response.into_reader().take(32 * 1024 * 1024));
        let mut done = false;
        for line in reader.lines() {
            let line = line?;
            let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if data == "[DONE]" {
                done = true;
                break;
            }
            let value = match serde_json::from_str::<Value>(data) {
                Ok(value) => value,
                Err(_) if self.provider == AiProvider::Xcli => {
                    return Err(AppError::Ai("xcli 返回了无效流事件，结果未完成".into()))
                }
                Err(_) => continue,
            };
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                if self.provider == AiProvider::Xcli {
                    return Err(AppError::Ai("xcli 流式请求失败，已保留未完成状态".into()));
                }
                return Err(AppError::Ai(format!("模型服务返回错误：{message}")));
            }
            let delta = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let visible = filter.push(delta);
            output.push_str(&visible);
            if !on_delta(&visible) {
                return Err(cancelled());
            }
        }
        if self.provider == AiProvider::Xcli && !done {
            return Err(AppError::Ai("xcli 响应意外中断，结果未完成".into()));
        }
        let rest = filter.finish();
        if !rest.is_empty() {
            output.push_str(&rest);
            if !on_delta(&rest) {
                return Err(cancelled());
            }
        }
        Ok(output.trim().to_string())
    }
}

impl LlmClient for OpenAiCompatClient {
    fn chat(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        self.complete(messages, None, on_delta)
    }

    fn chat_limited(
        &self,
        messages: &[ChatMessage],
        max_tokens: u32,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        self.complete(messages, Some(max_tokens), on_delta)
    }

    fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        self.embeddings(inputs, true)
    }

    #[allow(clippy::misnamed_getters)]
    fn embed_model(&self) -> &str {
        &self.index_model
    }

    fn validate_embed_model(&self) -> AppResult<()> {
        if self.provider == AiProvider::Xcli
            && self.approved_identity.as_ref() != Some(&self.current_embedding_identity()?)
        {
            return Err(gateway::rebuild_required());
        }
        Ok(())
    }
}

impl OpenAiCompatClient {
    fn embeddings(&self, inputs: &[String], require_index: bool) -> AppResult<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let identity = if self.provider == AiProvider::Xcli {
            let identity = self.current_embedding_identity()?;
            if require_index && self.approved_identity.as_ref() != Some(&identity) {
                return Err(gateway::rebuild_required());
            }
            Some(identity)
        } else {
            None
        };
        let body = self
            .post(
                "/embeddings",
                &json!({ "model": self.embed_model, "input": inputs, "encoding_format": "float" }),
            )?
            .into_string()?;
        let value: Value = serde_json::from_str(&body)?;
        if let Some(identity) = &identity {
            identity.check_response(&value)?;
        }
        let mut rows: Vec<(usize, Vec<f32>)> = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AppError::Ai("向量模型返回了无法识别的结果".into()))?
            .iter()
            .enumerate()
            .map(|(position, item)| -> AppResult<(usize, Vec<f32>)> {
                let index = item
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|index| index as usize)
                    .unwrap_or(position);
                let vector: Vec<f32> = item
                    .get("embedding")
                    .and_then(Value::as_array)
                    .ok_or_else(|| AppError::Ai("向量模型返回了无效向量".into()))?
                    .iter()
                    .map(|value| {
                        let value = value
                            .as_f64()
                            .ok_or_else(|| AppError::Ai("向量模型返回了非数值分量".into()))?;
                        let component = value as f32;
                        if !component.is_finite() {
                            return Err(AppError::Ai("向量模型返回了非有限分量".into()));
                        }
                        Ok(component)
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                if identity
                    .as_ref()
                    .is_some_and(|identity| identity.dimensions != vector.len())
                {
                    return Err(AppError::Ai("xcli 向量维度与模型身份不匹配".into()));
                }
                Ok((index, normalize(vector)))
            })
            .collect::<AppResult<Vec<_>>>()?;
        rows.sort_by_key(|(index, _)| *index);
        if rows.len() != inputs.len()
            || rows
                .iter()
                .enumerate()
                .any(|(i, (index, vector))| *index != i || vector.is_empty())
        {
            return Err(AppError::Ai("向量模型返回的结果数量或顺序不完整".into()));
        }
        let dimensions = rows.first().map(|(_, row)| row.len()).unwrap_or(0);
        if rows.iter().any(|(_, row)| row.len() != dimensions) {
            return Err(AppError::Ai("向量结果的维度不一致".into()));
        }
        Ok(rows.into_iter().map(|(_, vector)| vector).collect())
    }
}

pub fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        vector
            .iter_mut()
            .for_each(|value| *value = (f64::from(*value) / norm) as f32);
    }
    vector
}

/// Hides `<think>…</think>` reasoning blocks that some local models emit inline.
#[derive(Default)]
pub struct ThinkFilter {
    pending: String,
    thinking: bool,
}

impl ThinkFilter {
    const OPEN: &'static str = "<think>";
    const CLOSE: &'static str = "</think>";

    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut visible = String::new();
        loop {
            let marker = if self.thinking {
                Self::CLOSE
            } else {
                Self::OPEN
            };
            if let Some(position) = self.pending.find(marker) {
                if !self.thinking {
                    visible.push_str(&self.pending[..position]);
                }
                self.pending.drain(..position + marker.len());
                self.thinking = !self.thinking;
                continue;
            }
            // Keep a possible partial marker at the end for the next chunk.
            let keep = (1..marker.len())
                .rev()
                .find(|length| self.pending.ends_with(&marker[..*length]))
                .unwrap_or(0);
            let flush = self.pending.len() - keep;
            if !self.thinking {
                visible.push_str(&self.pending[..flush]);
            }
            self.pending.drain(..flush);
            break;
        }
        visible
    }

    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        if self.thinking {
            String::new()
        } else {
            rest
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic client: embeds by hashing characters, replies with canned text.
    pub struct FakeClient {
        pub replies: Mutex<Vec<String>>,
        pub prompts: Mutex<Vec<String>>,
    }

    impl FakeClient {
        pub fn new(replies: &[&str]) -> Self {
            Self {
                replies: Mutex::new(replies.iter().rev().map(|item| item.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    pub fn fake_vector(text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; 16];
        for character in text.chars() {
            vector[(character as usize) % 16] += 1.0;
        }
        normalize(vector)
    }

    impl LlmClient for FakeClient {
        fn chat(
            &self,
            messages: &[ChatMessage],
            on_delta: &mut dyn FnMut(&str) -> bool,
        ) -> AppResult<String> {
            self.prompts.lock().unwrap().push(
                messages
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_else(|| "好的".into());
            if !on_delta(&reply) {
                return Err(cancelled());
            }
            Ok(reply)
        }

        fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
            Ok(inputs.iter().map(|input| fake_vector(input)).collect())
        }

        fn embed_model(&self) -> &str {
            "fake-embed"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::mpsc;

    struct GatewayFixture {
        url: String,
        posts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        consent_headers: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fingerprint: std::sync::Arc<std::sync::Mutex<String>>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for GatewayFixture {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    fn gateway_fixture(location: &str, valid_catalog: bool) -> GatewayFixture {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let posts = Arc::new(AtomicUsize::new(0));
        let consent_headers = Arc::new(AtomicUsize::new(0));
        let fingerprint = Arc::new(Mutex::new("a".repeat(64)));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_posts, thread_headers, thread_fingerprint, thread_stop) = (
            posts.clone(),
            consent_headers.clone(),
            fingerprint.clone(),
            stop.clone(),
        );
        let location = location.to_string();
        let worker = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let method = line.split_whitespace().next().unwrap().to_string();
                let path = line.split_whitespace().nth(1).unwrap().to_string();
                let mut length = 0usize;
                let mut consent = false;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("content-length:") {
                        length = lower.split(':').nth(1).unwrap().trim().parse().unwrap();
                    }
                    if lower.starts_with("x-xcli-remote-consent:") {
                        consent = lower.contains("allow");
                    }
                }
                let mut request = vec![0; length];
                reader.read_exact(&mut request).unwrap();
                if method == "POST" {
                    thread_posts.fetch_add(1, Ordering::SeqCst);
                    if consent {
                        thread_headers.fetch_add(1, Ordering::SeqCst);
                    }
                }
                let fingerprint = thread_fingerprint.lock().unwrap().clone();
                let body = if path == "/v1/models" && valid_catalog {
                    json!({"object":"list","data":[
                        {"id":"chat","xcli":{"capabilities":["text.generate","text.stream"],"inference_location":location}},
                        {"id":"embed","xcli":{"capabilities":["embeddings"],"inference_location":location,"dimensions":3,"index_version":"fixture-v1","index_fingerprint":fingerprint}}
                    ]}).to_string()
                } else if path == "/v1/embeddings" {
                    json!({"object":"list","data":[{"object":"embedding","index":0,"embedding":[3,4,0]}],"xcli":{"dimensions":3,"index_version":"fixture-v1","index_fingerprint":fingerprint}}).to_string()
                } else if path == "/v1/chat/completions" {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"synthetic\"}}]}\n\ndata: [DONE]\n\n".into()
                } else {
                    "not a model catalog".into()
                };
                let typ = if path == "/v1/chat/completions" {
                    "text/event-stream"
                } else {
                    "application/json"
                };
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: {typ}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        GatewayFixture {
            url,
            posts,
            consent_headers,
            fingerprint,
            stop,
            worker: Some(worker),
        }
    }

    fn gateway_client(
        fixture: &GatewayFixture,
        allow_cloud: bool,
        consent: bool,
    ) -> OpenAiCompatClient {
        OpenAiCompatClient::new(
            &AiSettings {
                provider: AiProvider::Xcli,
                enabled: true,
                base_url: fixture.url.clone(),
                chat_model: "chat".into(),
                embed_model: "embed".into(),
                allow_cloud,
                ..AiSettings::default()
            },
            Some("synthetic-app-token".into()),
        )
        .with_cloud_consent(consent)
    }

    #[test]
    fn xcli_remote_unknown_or_invalid_catalog_never_receives_chat_without_consent() {
        use std::sync::atomic::Ordering;
        for (location, valid, allow, consent) in [
            ("remote", true, true, false),
            ("unknown", true, true, false),
            ("remote", true, false, true),
            ("local", false, true, true),
        ] {
            let fixture = gateway_fixture(location, valid);
            let result = gateway_client(&fixture, allow, consent).chat(
                &[ChatMessage::user("synthetic private canary")],
                &mut |_| true,
            );
            assert!(result.is_err());
            assert_eq!(fixture.posts.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn xcli_local_and_explicit_remote_consent_use_distinct_http_permissions() {
        use std::sync::atomic::Ordering;
        for (location, allow, consent, headers) in
            [("local", false, false, 0), ("remote", true, true, 1)]
        {
            let fixture = gateway_fixture(location, true);
            assert_eq!(
                gateway_client(&fixture, allow, consent)
                    .chat(&[ChatMessage::user("synthetic")], &mut |_| true)
                    .unwrap(),
                "synthetic"
            );
            assert_eq!(fixture.posts.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.consent_headers.load(Ordering::SeqCst), headers);
        }
    }

    #[test]
    fn xcli_model_changes_require_manual_rebuild_before_any_embedding_upload() {
        use std::sync::atomic::Ordering;
        let fixture = gateway_fixture("local", true);
        let mut client = gateway_client(&fixture, false, false);
        assert!(client.embed(&["synthetic".into()]).is_err());
        assert_eq!(fixture.posts.load(Ordering::SeqCst), 0);
        assert!(client.prepare_index(false).is_err());
        let first = client.prepare_index(true).unwrap().unwrap();
        assert_eq!(client.embed_model(), first.key());
        assert_eq!(
            client.embed(&["synthetic".into()]).unwrap(),
            vec![vec![0.6, 0.8, 0.0]]
        );
        *fixture.fingerprint.lock().unwrap() = "b".repeat(64);
        assert!(client.validate_embed_model().is_err());
        assert!(client.embed(&["synthetic".into()]).is_err());
        assert_eq!(fixture.posts.load(Ordering::SeqCst), 1);
        assert!(client.prepare_index(false).is_err());
        let second = client.prepare_index(true).unwrap().unwrap();
        assert_ne!(first.key(), second.key());
        assert!(client.embed(&["synthetic".into()]).is_ok());
        assert_eq!(fixture.posts.load(Ordering::SeqCst), 2);
    }

    /// Answers a single HTTP request with `response` and reports the request body.
    fn serve_once(response: String) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let _ = sender.send(String::from_utf8(body).unwrap());
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}/v1"), receiver)
    }

    fn client(base_url: String) -> OpenAiCompatClient {
        OpenAiCompatClient::new(
            &AiSettings {
                base_url,
                ..AiSettings::default()
            },
            Some("sk-test".into()),
        )
    }

    #[test]
    fn streams_server_sent_events() {
        let events = [
            r#"{"choices":[{"delta":{"role":"assistant"}}]}"#,
            r#"{"choices":[{"delta":{"content":"<think>先想想</think>你好"}}]}"#,
            r#"{"choices":[{"delta":{"content":"，世界"}}]}"#,
        ]
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        let (url, request) = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{events}"
        ));
        let mut deltas = Vec::new();
        let reply = client(url)
            .chat(&[ChatMessage::user("打个招呼")], &mut |delta| {
                deltas.push(delta.to_string());
                true
            })
            .unwrap();
        assert_eq!(reply, "你好，世界");
        assert_eq!(deltas.concat(), "你好，世界");
        let body: Value = serde_json::from_str(&request.recv().unwrap()).unwrap();
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["content"], "打个招呼");
    }

    #[test]
    fn limits_tokens_and_can_stop_during_hidden_reasoning() {
        let events = [
            r#"{"choices":[{"delta":{"reasoning":"先想想"}}]}"#,
            r#"{"choices":[{"delta":{"reasoning":"再想想"}}]}"#,
            r#"{"choices":[{"delta":{"content":"答案"}}]}"#,
        ]
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
        let (url, request) = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{events}"
        ));
        let mut calls = 0;
        let error = client(url)
            .chat_limited(&[ChatMessage::user("问")], 512, &mut |delta| {
                calls += 1;
                assert!(delta.is_empty());
                calls < 2
            })
            .unwrap_err();
        assert_eq!(error.to_string(), cancelled().to_string());
        let body: Value = serde_json::from_str(&request.recv().unwrap()).unwrap();
        assert_eq!(body["max_tokens"], 512);
    }

    #[test]
    fn parses_and_normalizes_embeddings() {
        let body = r#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[3,4]}]}"#;
        let (url, _) = serve_once(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let vectors = client(url).embed(&["a".into(), "b".into()]).unwrap();
        assert_eq!(vectors, vec![vec![0.6, 0.8], vec![0.0, 1.0]]);
    }

    #[test]
    fn explains_missing_models() {
        let body = r#"{"error":{"message":"model \"bge-m3\" not found, try pulling it first"}}"#;
        let (url, _) = serve_once(format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let error = client(url).embed(&["a".into()]).unwrap_err().to_string();
        assert!(
            error.contains("404") && error.contains("not found"),
            "{error}"
        );
    }

    #[test]
    fn reports_unreachable_service() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        drop(listener);
        let error = client(url).embed(&["a".into()]).unwrap_err().to_string();
        assert!(error.contains("无法连接模型服务"), "{error}");
    }

    #[test]
    fn strips_think_blocks_across_chunks() {
        let mut filter = ThinkFilter::default();
        let mut output = String::new();
        for chunk in [
            "<thi",
            "nk>推理",
            "过程</th",
            "ink>\n答案",
            "是 42<",
            "b>!<",
        ] {
            output.push_str(&filter.push(chunk));
        }
        output.push_str(&filter.finish());
        assert_eq!(output, "\n答案是 42<b>!<");
    }

    #[test]
    fn normalizes_vectors() {
        let vector = normalize(vec![3.0, 4.0]);
        assert!((vector[0] - 0.6).abs() < 1e-6 && (vector[1] - 0.8).abs() < 1e-6);
        assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }
}
