use super::settings::AiSettings;
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
    fn chat(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String>;

    /// Returns one L2-normalized vector per input, in input order.
    fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>>;

    fn embed_model(&self) -> &str;
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
}

impl OpenAiCompatClient {
    pub fn new(settings: &AiSettings, api_key: Option<String>) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(300))
                .timeout_write(Duration::from_secs(60))
                .build(),
            base_url: settings.base_url.trim_end_matches('/').to_string(),
            api_key,
            chat_model: settings.chat_model.clone(),
            embed_model: settings.embed_model.clone(),
        }
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
        self.request("POST", path)
            .send_string(&body.to_string())
            .map_err(|error| self.map_error(error))
    }

    fn map_error(&self, error: ureq::Error) -> AppError {
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

impl LlmClient for OpenAiCompatClient {
    fn chat(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        let response = self.post(
            "/chat/completions",
            &json!({
                "model": self.chat_model,
                "messages": messages,
                "stream": true,
                "temperature": 0.3,
            }),
        )?;
        let is_event_stream = response
            .header("Content-Type")
            .is_some_and(|value| value.contains("text/event-stream"));
        let mut filter = ThinkFilter::default();
        let mut output = String::new();
        if !is_event_stream {
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
        let reader = BufReader::new(response.into_reader());
        for line in reader.lines() {
            let line = line?;
            let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if data == "[DONE]" {
                break;
            }
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
                return Err(AppError::Ai(format!("模型服务返回错误：{message}")));
            }
            let Some(delta) = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let visible = filter.push(delta);
            if visible.is_empty() {
                continue;
            }
            output.push_str(&visible);
            if !on_delta(&visible) {
                return Err(cancelled());
            }
        }
        let rest = filter.finish();
        if !rest.is_empty() {
            output.push_str(&rest);
            on_delta(&rest);
        }
        Ok(output.trim().to_string())
    }

    fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let body = self
            .post(
                "/embeddings",
                &json!({ "model": self.embed_model, "input": inputs }),
            )?
            .into_string()?;
        let value: Value = serde_json::from_str(&body)?;
        let mut rows: Vec<(usize, Vec<f32>)> = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AppError::Ai("向量模型返回了无法识别的结果".into()))?
            .iter()
            .enumerate()
            .map(|(position, item)| {
                let index = item
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|index| index as usize)
                    .unwrap_or(position);
                let vector = item
                    .get("embedding")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_f64)
                            .map(|value| value as f32)
                            .collect()
                    })
                    .unwrap_or_default();
                (index, normalize(vector))
            })
            .collect();
        rows.sort_by_key(|(index, _)| *index);
        if rows.len() != inputs.len() || rows.iter().any(|(_, vector)| vector.is_empty()) {
            return Err(AppError::Ai("向量模型返回的结果数量不完整".into()));
        }
        Ok(rows.into_iter().map(|(_, vector)| vector).collect())
    }

    fn embed_model(&self) -> &str {
        &self.embed_model
    }
}

pub fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        vector.iter_mut().for_each(|value| *value /= norm);
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
