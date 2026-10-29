//! Model Gateway metadata and index identities. A loopback transport is not proof
//! that inference runs locally; unknown locations are treated as cloud inference.
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;

pub fn endpoint(value: &str) -> AppResult<String> {
    let bad = || AppError::Validation("xcli 需要明确的本机 IP、端口和 /v1 路径".into());
    let rest = value.strip_prefix("http://").ok_or_else(bad)?;
    let (authority, path) = rest.split_once('/').ok_or_else(bad)?;
    let address: SocketAddr = authority.parse().map_err(|_| bad())?;
    if !address.ip().is_loopback() || address.port() == 0 || path.trim_end_matches('/') != "v1" {
        return Err(bad());
    }
    Ok(format!("http://{address}/v1"))
}

pub fn selected<'a>(catalog: &'a Value, alias: &str, capability: &str) -> AppResult<&'a Value> {
    let models = catalog
        .get("data")
        .and_then(Value::as_array)
        .filter(|_| catalog.get("object").and_then(Value::as_str) == Some("list"))
        .ok_or_else(|| AppError::Ai("无法验证 xcli 模型目录；未发送聊天内容".into()))?;
    let matches: Vec<_> = models
        .iter()
        .filter(|item| item.get("id").and_then(Value::as_str) == Some(alias))
        .collect();
    if matches.len() != 1
        || !matches[0]
            .pointer("/xcli/capabilities")
            .and_then(Value::as_array)
            .is_some_and(|caps| caps.iter().any(|cap| cap.as_str() == Some(capability)))
    {
        return Err(AppError::Ai(
            "xcli 未提供所选模型或所需能力；未发送聊天内容".into(),
        ));
    }
    Ok(matches[0])
}

pub fn effective_cloud(model: &Value) -> bool {
    model
        .pointer("/xcli/inference_location")
        .and_then(Value::as_str)
        != Some("local")
}

pub fn ensure_consent(model: &Value, allow_cloud: bool, consent: bool) -> AppResult<()> {
    if effective_cloud(model) && (!allow_cloud || !consent) {
        return Err(AppError::Ai(
            "xcli 上游在云端或位置未知；请先允许云端服务并确认本次传输，聊天内容尚未发送".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingIdentity {
    pub endpoint: String,
    pub alias: String,
    pub dimensions: usize,
    pub version: String,
    pub fingerprint: String,
}

impl EmbeddingIdentity {
    pub fn from_model(base_url: &str, alias: &str, model: &Value) -> AppResult<Self> {
        let dimensions = model
            .pointer("/xcli/dimensions")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let version = model
            .pointer("/xcli/index_version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let fingerprint = model
            .pointer("/xcli/index_fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(1..=65536).contains(&dimensions)
            || version.is_empty()
            || version.len() > 128
            || fingerprint.len() != 64
            || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(AppError::Ai(
                "xcli 向量模型缺少可信维度或索引身份；未使用旧索引".into(),
            ));
        }
        Ok(Self {
            endpoint: endpoint(base_url)?,
            alias: alias.into(),
            dimensions: dimensions as usize,
            version: version.into(),
            fingerprint: fingerprint.into(),
        })
    }

    pub fn key(&self) -> String {
        format!(
            "{}|xcli:{}:{}:{}",
            self.alias, self.dimensions, self.version, self.fingerprint
        )
    }

    pub fn check_response(&self, response: &Value) -> AppResult<()> {
        if response
            .pointer("/xcli/index_fingerprint")
            .and_then(Value::as_str)
            != Some(&self.fingerprint)
            || response.pointer("/xcli/dimensions").and_then(Value::as_u64)
                != Some(self.dimensions as u64)
            || response
                .pointer("/xcli/index_version")
                .and_then(Value::as_str)
                != Some(&self.version)
        {
            return Err(AppError::Ai(
                "xcli 向量路由在请求中发生变化；结果已丢弃，请显式重建索引".into(),
            ));
        }
        Ok(())
    }
}

pub fn rebuild_required() -> AppError {
    AppError::Ai(
        "xcli 向量模型首次接入或身份已变化，请在 AI 设置中显式重建索引；旧索引保持不变".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_routes_are_cloud_and_loopback_cannot_grant_consent() {
        for location in ["remote", "unknown", "invalid"] {
            let model = json!({"xcli":{"inference_location":location}});
            assert!(effective_cloud(&model));
            assert!(ensure_consent(&model, true, false).is_err());
            assert!(ensure_consent(&model, false, true).is_err());
            assert!(ensure_consent(&model, true, true).is_ok());
        }
        assert!(effective_cloud(&json!({})));
        assert!(ensure_consent(
            &json!({"xcli":{"inference_location":"local"}}),
            false,
            false
        )
        .is_ok());
        for value in [
            "http://localhost:18741/v1",
            "http://127.0.0.1:18741/v1?token=canary",
            "http://user@127.0.0.1:18741/v1",
            "http://192.168.1.1:18741/v1",
            "http://127.0.0.1:18741/v1/other",
        ] {
            assert!(endpoint(value).is_err());
        }
    }

    #[test]
    fn model_and_dimension_and_provider_changes_get_separate_index_namespaces() {
        let meta = json!({"xcli":{"dimensions":3,"index_version":"v1","index_fingerprint":"a".repeat(64)}});
        let old =
            EmbeddingIdentity::from_model("http://127.0.0.1:18741/v1", "embed", &meta).unwrap();
        let mut changed = meta.clone();
        changed["xcli"]["dimensions"] = json!(4);
        assert_ne!(
            old.key(),
            EmbeddingIdentity::from_model(&old.endpoint, "embed", &changed)
                .unwrap()
                .key()
        );
        changed["xcli"]["index_fingerprint"] = json!("b".repeat(64));
        assert_ne!(
            old.key(),
            EmbeddingIdentity::from_model(&old.endpoint, "embed", &changed)
                .unwrap()
                .key()
        );
        assert_ne!(
            old.key(),
            EmbeddingIdentity::from_model(&old.endpoint, "other", &meta)
                .unwrap()
                .key()
        );
        assert!(old.check_response(&json!({"xcli":{"dimensions":4,"index_version":"v1","index_fingerprint":"a".repeat(64)}})).is_err());
    }
}
