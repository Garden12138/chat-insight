use crate::error::{AppError, AppResult};
use crate::wechat_account::write_private_file;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/v1";
pub const BAILIAN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
pub const BAILIAN_CHAT_MODEL: &str = "qwen-plus";
pub const BAILIAN_EMBED_MODEL: &str = "text-embedding-v3";

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    OpenaiCompatible,
    Bailian,
    Xcli,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub provider: AiProvider,
    pub enabled: bool,
    pub base_url: String,
    pub chat_model: String,
    pub embed_model: String,
    pub allow_cloud: bool,
    /// Scan new messages for commitments after each sync. Only honoured for local models,
    /// because cloud requests need the user's consent every time.
    pub auto_scan_commitments: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: AiProvider::OpenaiCompatible,
            enabled: false,
            base_url: DEFAULT_BASE_URL.into(),
            chat_model: "qwen3:8b".into(),
            embed_model: "bge-m3".into(),
            allow_cloud: false,
            auto_scan_commitments: false,
        }
    }
}

impl AiSettings {
    pub fn is_local(&self) -> bool {
        // Named cloud vendors are never local. Gateway locality is decided at send time.
        !matches!(self.provider, AiProvider::Bailian | AiProvider::Xcli)
            && is_local_endpoint(&self.base_url)
    }

    /// Rejects requests the user has not allowed. `sends_content` marks requests that
    /// carry chat text (as opposed to aggregate statistics or the user's own query).
    pub fn ensure_allowed(&self, sends_content: bool, cloud_consent: bool) -> AppResult<()> {
        if !self.enabled {
            return Err(AppError::Ai(
                "AI 功能尚未开启，请先在“数据管理 → AI 设置”中开启".into(),
            ));
        }
        if self.provider == AiProvider::Xcli {
            return Ok(());
        }
        if self.is_local() {
            return Ok(());
        }
        if !self.allow_cloud {
            return Err(AppError::Ai(
                "模型服务地址不在本机。如需使用云端服务，请先在 AI 设置中允许".into(),
            ));
        }
        if sends_content && !cloud_consent {
            return Err(AppError::Ai(
                "需要你确认后才能把聊天内容发送到云端模型服务".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsView {
    #[serde(flatten)]
    pub settings: AiSettings,
    pub has_api_key: bool,
    pub is_local: bool,
    pub host: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsInput {
    #[serde(flatten)]
    pub settings: AiSettings,
    /// `None` keeps the stored key, an empty string removes it.
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("ai-settings.json")
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("api-key")
    }

    pub fn load(&self) -> AiSettings {
        fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn api_key(&self) -> Option<String> {
        fs::read_to_string(self.key_path())
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
    }

    pub fn embedding_identity(&self) -> Option<super::gateway::EmbeddingIdentity> {
        fs::read(self.dir.join("xcli-embedding-identity.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    pub fn accept_embedding_identity(
        &self,
        identity: &super::gateway::EmbeddingIdentity,
    ) -> AppResult<()> {
        write_private_file(
            &self.dir.join("xcli-embedding-identity.json"),
            &serde_json::to_vec(identity)?,
        )
    }

    pub fn view(&self) -> AiSettingsView {
        let settings = self.load();
        AiSettingsView {
            is_local: settings.is_local(),
            host: endpoint_host(&settings.base_url).unwrap_or_default(),
            has_api_key: self.api_key().is_some(),
            settings,
        }
    }

    pub fn save(&self, input: AiSettingsInput) -> AppResult<AiSettingsView> {
        let mut settings = input.settings;
        let previous = self.load();
        if settings.provider != previous.provider
            && vendor_switch_needs_new_key(previous.provider, settings.provider)
            && input.api_key.is_none()
            && self.api_key().is_some()
        {
            return Err(AppError::Validation(
                "切换云端服务商需要重新填写 API Key，不能复用其他供应商凭据".into(),
            ));
        }
        settings.base_url = settings.base_url.trim().trim_end_matches('/').to_string();
        settings.chat_model = settings.chat_model.trim().to_string();
        settings.embed_model = settings.embed_model.trim().to_string();
        if settings.provider == AiProvider::Bailian {
            if settings.base_url.is_empty() {
                settings.base_url = BAILIAN_BASE_URL.into();
            }
            settings.allow_cloud = true;
            if settings.chat_model.is_empty() {
                settings.chat_model = BAILIAN_CHAT_MODEL.into();
            }
            if settings.embed_model.is_empty() {
                settings.embed_model = BAILIAN_EMBED_MODEL.into();
            }
        }
        if settings.provider == AiProvider::Xcli {
            settings.base_url = super::gateway::endpoint(&settings.base_url)?;
        }
        if !(settings.base_url.starts_with("http://") || settings.base_url.starts_with("https://"))
            || endpoint_host(&settings.base_url).is_none()
        {
            return Err(AppError::Validation(
                "服务地址需要以 http:// 或 https:// 开头".into(),
            ));
        }
        if settings.chat_model.is_empty() || settings.embed_model.is_empty() {
            return Err(AppError::Validation("请填写对话模型和向量模型".into()));
        }
        let next_key = match input.api_key.as_deref().map(str::trim) {
            Some("") => None,
            Some(key) => Some(key.to_string()),
            None => self.api_key(),
        };
        if settings.provider == AiProvider::Bailian && settings.enabled && next_key.is_none() {
            return Err(AppError::Validation(
                "使用阿里云百炼需要填写 API Key".into(),
            ));
        }
        settings.auto_scan_commitments &= settings.is_local();
        write_private_file(
            &self.settings_path(),
            serde_json::to_string_pretty(&settings)?.as_bytes(),
        )?;
        match input.api_key.as_deref().map(str::trim) {
            Some("") => match fs::remove_file(self.key_path()) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into())
                }
                _ => {}
            },
            Some(key) => write_private_file(&self.key_path(), key.as_bytes())?,
            None => {}
        }
        Ok(self.view())
    }
}

pub fn endpoint_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn vendor_switch_needs_new_key(previous: AiProvider, next: AiProvider) -> bool {
    previous != next
        && matches!(
            (previous, next),
            (AiProvider::Bailian, _)
                | (_, AiProvider::Bailian)
                | (AiProvider::Xcli, _)
                | (_, AiProvider::Xcli)
        )
}

pub fn is_local_endpoint(url: &str) -> bool {
    matches!(
        endpoint_host(url).as_deref(),
        Some("localhost" | "127.0.0.1" | "::1")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn xcli_settings_do_not_reuse_provider_keys_or_enable_background_cloud_upload() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        store
            .save(AiSettingsInput {
                settings: AiSettings::default(),
                api_key: Some("old-provider-canary".into()),
            })
            .unwrap();
        let gateway = AiSettings {
            provider: AiProvider::Xcli,
            enabled: true,
            base_url: "http://127.0.0.1:18741/v1".into(),
            chat_model: "xcli/text-default".into(),
            embed_model: "xcli/embed-default".into(),
            auto_scan_commitments: true,
            ..AiSettings::default()
        };
        assert!(store
            .save(AiSettingsInput {
                settings: gateway.clone(),
                api_key: None
            })
            .is_err());
        assert_eq!(store.load().provider, AiProvider::OpenaiCompatible);
        assert_eq!(store.api_key().as_deref(), Some("old-provider-canary"));
        let view = store
            .save(AiSettingsInput {
                settings: gateway,
                api_key: Some("independent-gateway-token".into()),
            })
            .unwrap();
        assert!(!view.is_local);
        assert!(!view.settings.auto_scan_commitments);
        assert!(!serde_json::to_string(&view)
            .unwrap()
            .contains("independent-gateway-token"));
        let identity = super::super::gateway::EmbeddingIdentity {
            endpoint: "http://127.0.0.1:18741/v1".into(),
            alias: "xcli/embed-default".into(),
            dimensions: 3,
            version: "v1".into(),
            fingerprint: "a".repeat(64),
        };
        assert!(store.embedding_identity().is_none());
        store.accept_embedding_identity(&identity).unwrap();
        assert_eq!(store.embedding_identity(), Some(identity));
    }

    #[test]
    fn bailian_keeps_namespace_endpoint_and_requires_its_own_key() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        store
            .save(AiSettingsInput {
                settings: AiSettings::default(),
                api_key: Some("old-provider-canary".into()),
            })
            .unwrap();
        let namespace = "https://llm-demo.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";
        let bailian = AiSettings {
            provider: AiProvider::Bailian,
            enabled: true,
            base_url: namespace.into(),
            chat_model: BAILIAN_CHAT_MODEL.into(),
            embed_model: BAILIAN_EMBED_MODEL.into(),
            allow_cloud: false,
            auto_scan_commitments: true,
        };
        assert!(store
            .save(AiSettingsInput {
                settings: bailian.clone(),
                api_key: None
            })
            .is_err());
        let view = store
            .save(AiSettingsInput {
                settings: bailian,
                api_key: Some("sk-bailian".into()),
            })
            .unwrap();
        assert_eq!(view.settings.base_url, namespace);
        assert!(view.settings.allow_cloud);
        assert!(!view.is_local);
        assert!(!view.settings.auto_scan_commitments);
        assert_eq!(view.host, "llm-demo.cn-beijing.maas.aliyuncs.com");
        assert_eq!(store.api_key().as_deref(), Some("sk-bailian"));
        let defaulted = store
            .save(AiSettingsInput {
                settings: AiSettings {
                    provider: AiProvider::Bailian,
                    enabled: true,
                    base_url: String::new(),
                    chat_model: BAILIAN_CHAT_MODEL.into(),
                    embed_model: BAILIAN_EMBED_MODEL.into(),
                    allow_cloud: true,
                    auto_scan_commitments: false,
                },
                api_key: Some("sk-bailian".into()),
            })
            .unwrap();
        assert_eq!(defaulted.settings.base_url, BAILIAN_BASE_URL);
    }

    #[test]
    fn recognizes_loopback_endpoints_only() {
        assert!(is_local_endpoint("http://127.0.0.1:11434/v1"));
        assert!(is_local_endpoint("http://localhost:1234/v1"));
        assert!(is_local_endpoint("http://[::1]:8080"));
        assert!(is_local_endpoint(
            "HTTP://LOCALHOST/v1".to_lowercase().as_str()
        ));
        assert!(!is_local_endpoint("https://api.deepseek.com/v1"));
        assert!(!is_local_endpoint("http://127.0.0.1.evil.com/v1"));
        assert!(!is_local_endpoint("http://localhost@evil.com/v1"));
        assert!(!is_local_endpoint("http://192.168.1.10:11434/v1"));
        assert!(!is_local_endpoint("ftp://localhost"));
    }

    #[test]
    fn gate_requires_opt_in_and_consent_for_cloud() {
        let mut settings = AiSettings::default();
        assert!(settings.ensure_allowed(false, false).is_err());
        settings.enabled = true;
        assert!(settings.ensure_allowed(true, false).is_ok());

        settings.base_url = "https://api.example.com/v1".into();
        assert!(settings.ensure_allowed(false, true).is_err());
        settings.allow_cloud = true;
        assert!(settings.ensure_allowed(false, false).is_ok());
        assert!(settings.ensure_allowed(true, false).is_err());
        assert!(settings.ensure_allowed(true, true).is_ok());

        settings.provider = AiProvider::Bailian;
        settings.base_url = BAILIAN_BASE_URL.into();
        assert!(!settings.is_local());
        assert!(settings.ensure_allowed(false, false).is_ok());
        assert!(settings.ensure_allowed(true, false).is_err());
        assert!(settings.ensure_allowed(true, true).is_ok());
    }

    #[test]
    fn stores_settings_and_private_key() {
        let directory = tempdir().unwrap();
        let store = SettingsStore::new(directory.path().join("ai"));
        assert_eq!(store.load(), AiSettings::default());

        let settings = AiSettings {
            enabled: true,
            base_url: "https://api.example.com/v1/".into(),
            auto_scan_commitments: true,
            ..AiSettings::default()
        };
        let view = store
            .save(AiSettingsInput {
                settings,
                api_key: Some("sk-test".into()),
            })
            .unwrap();
        assert_eq!(view.settings.base_url, "https://api.example.com/v1");
        assert!(view.has_api_key && !view.is_local);
        assert!(!view.settings.auto_scan_commitments);
        let local = store
            .save(AiSettingsInput {
                settings: AiSettings {
                    auto_scan_commitments: true,
                    ..AiSettings::default()
                },
                api_key: None,
            })
            .unwrap();
        assert!(local.settings.auto_scan_commitments);
        store
            .save(AiSettingsInput {
                settings: view.settings.clone(),
                api_key: None,
            })
            .unwrap();
        assert_eq!(view.host, "api.example.com");
        assert_eq!(store.api_key().as_deref(), Some("sk-test"));

        let kept = store
            .save(AiSettingsInput {
                settings: store.load(),
                api_key: None,
            })
            .unwrap();
        assert!(kept.has_api_key);
        let cleared = store
            .save(AiSettingsInput {
                settings: store.load(),
                api_key: Some(String::new()),
            })
            .unwrap();
        assert!(!cleared.has_api_key);
    }
}
