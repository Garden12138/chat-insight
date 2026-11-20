use super::xcli::XCLI_AGENT;
use crate::error::{AppError, AppResult};
use crate::wechat_account::write_private_file;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/v1";
pub const BAILIAN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
pub const BAILIAN_CHAT_MODEL: &str = "qwen-plus";
pub const BAILIAN_EMBED_MODEL: &str = "text-embedding-v3";
const LEGACY_IDENTITY_FILE: &str = "xcli-embedding-identity.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    OpenaiCompatible,
    Bailian,
    Xcli,
}

#[derive(Debug, Clone, Copy)]
pub enum AiTarget {
    Text,
    Vectors,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub provider: AiProvider,
    pub enabled: bool,
    pub base_url: String,
    pub chat_model: String,
    pub embed_model: String,
    pub vector_provider: Option<AiProvider>,
    pub vector_base_url: String,
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
            vector_provider: None,
            vector_base_url: DEFAULT_BASE_URL.into(),
            allow_cloud: false,
            auto_scan_commitments: false,
        }
    }
}

impl AiSettings {
    pub fn is_local(&self) -> bool {
        self.provider == AiProvider::OpenaiCompatible && is_local_endpoint(&self.base_url)
    }

    pub fn is_local_target(&self, target: AiTarget) -> bool {
        match (self.provider, target) {
            (AiProvider::Xcli, AiTarget::Vectors) => match self.vector_provider {
                None => true,
                Some(AiProvider::OpenaiCompatible) => is_local_endpoint(&self.vector_base_url),
                Some(_) => false,
            },
            _ => self.is_local(),
        }
    }

    pub fn ensure_allowed(&self, sends_content: bool, cloud_consent: bool) -> AppResult<()> {
        self.ensure_allowed_for(AiTarget::Text, sends_content, cloud_consent)?;
        self.ensure_allowed_for(AiTarget::Vectors, sends_content, cloud_consent)
    }

    /// Checks the service receiving this request. `sends_content` marks requests that carry
    /// chat text, as opposed to aggregate statistics or the user's own query.
    pub fn ensure_allowed_for(
        &self,
        target: AiTarget,
        sends_content: bool,
        cloud_consent: bool,
    ) -> AppResult<()> {
        if !self.enabled {
            return Err(AppError::Ai(
                "AI 功能尚未开启，请先在“数据管理 → AI 设置”中开启".into(),
            ));
        }
        if self.is_local_target(target) {
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
    pub has_vector_api_key: bool,
    pub text_is_local: bool,
    pub vectors_is_local: bool,
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
    /// `None` keeps the stored vector key, an empty string removes it.
    pub vector_api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SettingsStore {
    dir: PathBuf,
    settings_lock: Arc<Mutex<()>>,
}

impl SettingsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            settings_lock: Arc::default(),
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.dir.join("ai-settings.json")
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("api-key")
    }

    fn vector_key_path(&self) -> PathBuf {
        self.dir.join("vector-api-key")
    }

    pub fn load(&self) -> AiSettings {
        let _guard = self
            .settings_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.load_and_migrate()
    }

    fn load_and_migrate(&self) -> AiSettings {
        let path = self.settings_path();
        let mut settings: AiSettings = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        if settings.provider != AiProvider::Xcli {
            return settings;
        }
        settings.chat_model = XCLI_AGENT.into();
        if settings.base_url.starts_with("http://") || settings.base_url.starts_with("https://") {
            settings.base_url = "xcli".into();
            settings.vector_provider = None;
            // Leave the migration pending on disk if the legacy identity could not be removed.
            if remove_file_if_present(self.dir.join(LEGACY_IDENTITY_FILE)).is_ok() {
                if let Ok(bytes) = serde_json::to_vec_pretty(&settings) {
                    let _ = write_private_file(&path, &bytes);
                }
            }
        }
        settings
    }

    pub fn api_key(&self) -> Option<String> {
        read_key(self.key_path())
    }

    pub fn vector_api_key(&self) -> Option<String> {
        read_key(self.vector_key_path())
    }

    pub(super) fn load_for_client(&self) -> (AiSettings, Option<String>) {
        let _guard = self
            .settings_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let settings = self.load_and_migrate();
        let key = match settings.provider {
            AiProvider::Xcli => self.vector_api_key(),
            _ => self.api_key(),
        };
        (settings, key)
    }

    pub fn view(&self) -> AiSettingsView {
        let _guard = self
            .settings_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.view_with(self.load_and_migrate())
    }

    fn view_with(&self, settings: AiSettings) -> AiSettingsView {
        AiSettingsView {
            is_local: settings.is_local(),
            text_is_local: settings.is_local_target(AiTarget::Text),
            vectors_is_local: settings.is_local_target(AiTarget::Vectors),
            host: endpoint_host(&settings.base_url).unwrap_or_default(),
            has_api_key: self.api_key().is_some(),
            has_vector_api_key: self.vector_api_key().is_some(),
            settings,
        }
    }

    pub fn save(&self, input: AiSettingsInput) -> AppResult<AiSettingsView> {
        let _guard = self
            .settings_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut settings = input.settings;
        let previous = self.load_and_migrate();
        if settings.vector_provider == Some(AiProvider::Xcli) {
            return Err(AppError::Validation("xcli 不能用作向量服务".into()));
        }
        if settings.provider != AiProvider::Xcli
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
            if settings.base_url.is_empty() {
                settings.base_url = "xcli".into();
            }
            settings.chat_model = XCLI_AGENT.into();
            settings.vector_base_url = settings.vector_base_url.trim().trim_end_matches('/').into();
            let bailian_vectors = settings.vector_provider == Some(AiProvider::Bailian);
            if settings.vector_base_url.is_empty() {
                settings.vector_base_url = if bailian_vectors {
                    BAILIAN_BASE_URL
                } else {
                    DEFAULT_BASE_URL
                }
                .into();
            }
            if settings.vector_provider.is_some() {
                validate_endpoint(&settings.vector_base_url)?;
            }
            if settings.embed_model.is_empty() {
                settings.embed_model = if bailian_vectors {
                    BAILIAN_EMBED_MODEL
                } else {
                    "bge-m3"
                }
                .into();
            }
        } else {
            validate_endpoint(&settings.base_url)?;
            if settings.chat_model.is_empty() || settings.embed_model.is_empty() {
                return Err(AppError::Validation("请填写对话模型和向量模型".into()));
            }
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
        update_key(self.key_path(), input.api_key.as_deref())?;
        update_key(self.vector_key_path(), input.vector_api_key.as_deref())?;
        Ok(self.view_with(settings))
    }
}

fn read_key(path: PathBuf) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn update_key(path: PathBuf, key: Option<&str>) -> AppResult<()> {
    match key.map(str::trim) {
        Some("") => remove_file_if_present(path),
        Some(key) => write_private_file(&path, key.as_bytes()),
        None => Ok(()),
    }
}

fn remove_file_if_present(path: PathBuf) -> AppResult<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

fn validate_endpoint(url: &str) -> AppResult<()> {
    if endpoint_host(url).is_none() {
        return Err(AppError::Validation(
            "服务地址需要以 http:// 或 https:// 开头".into(),
        ));
    }
    Ok(())
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
    fn migrates_saved_xcli_settings_and_removes_legacy_identity() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        let identity_path = store.dir.join(LEGACY_IDENTITY_FILE);
        fs::create_dir(&store.dir).unwrap();
        fs::write(
            store.settings_path(),
            br#"{"provider":"xcli","baseUrl":"http://127.0.0.1:8080/v1","chatModel":"xcli/text-default","vectorProvider":"bailian"}"#,
        )
        .unwrap();
        fs::write(&identity_path, b"synthetic identity").unwrap();

        let settings = store.load();
        assert_eq!(settings.base_url, "xcli");
        assert_eq!(settings.chat_model, "codex");
        assert_eq!(settings.vector_provider, None);
        assert_eq!(settings.vector_base_url, DEFAULT_BASE_URL);
        assert!(!identity_path.exists());
        let saved = fs::read_to_string(store.settings_path()).unwrap();
        assert_eq!(
            serde_json::from_str::<AiSettings>(&saved).unwrap(),
            settings
        );
        assert_eq!(store.load(), settings);
    }

    #[test]
    fn loads_saved_xcli_choices_as_codex_without_changing_the_vector_service() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        fs::create_dir(&store.dir).unwrap();
        for model in ["auto", "antigravity", "cursor", "codex"] {
            let stored = AiSettings {
                provider: AiProvider::Xcli,
                base_url: "xcli".into(),
                chat_model: model.into(),
                vector_provider: Some(AiProvider::Bailian),
                ..AiSettings::default()
            };
            fs::write(store.settings_path(), serde_json::to_vec(&stored).unwrap()).unwrap();
            let settings = store.load();
            assert_eq!(settings.chat_model, "codex");
            assert_eq!(settings.vector_provider, Some(AiProvider::Bailian));
        }
    }

    #[test]
    fn xcli_checks_text_and_vector_destinations_independently() {
        let mut settings = AiSettings {
            provider: AiProvider::Xcli,
            base_url: "xcli".into(),
            enabled: true,
            vector_provider: Some(AiProvider::OpenaiCompatible),
            ..AiSettings::default()
        };

        assert!(!settings.is_local_target(AiTarget::Text));
        assert!(settings.is_local_target(AiTarget::Vectors));
        assert!(settings
            .ensure_allowed_for(AiTarget::Vectors, true, false)
            .is_ok());
        assert!(settings
            .ensure_allowed_for(AiTarget::Text, true, true)
            .is_err());

        settings.allow_cloud = true;
        assert!(settings
            .ensure_allowed_for(AiTarget::Text, true, false)
            .is_err());
        assert!(settings
            .ensure_allowed_for(AiTarget::Text, true, true)
            .is_ok());
        assert!(settings.ensure_allowed(true, false).is_err());
        assert!(settings.ensure_allowed(true, true).is_ok());

        settings.vector_base_url = "https://vectors.example.com/v1".into();
        assert!(!settings.is_local_target(AiTarget::Vectors));
        settings.vector_base_url = DEFAULT_BASE_URL.into();
        settings.vector_provider = Some(AiProvider::Bailian);
        assert!(!settings.is_local_target(AiTarget::Vectors));
        settings.vector_provider = None;
        assert!(settings.is_local_target(AiTarget::Vectors));

        settings.base_url = DEFAULT_BASE_URL.into();
        for provider in [AiProvider::OpenaiCompatible, AiProvider::Bailian] {
            settings.provider = provider;
            assert_eq!(
                settings.is_local_target(AiTarget::Text),
                settings.is_local()
            );
            assert_eq!(
                settings.is_local_target(AiTarget::Vectors),
                settings.is_local()
            );
        }
    }

    #[test]
    fn stores_vector_key_separately_and_applies_xcli_defaults() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        store
            .save(AiSettingsInput {
                settings: AiSettings::default(),
                api_key: Some("synthetic-text-key".into()),
                vector_api_key: None,
            })
            .unwrap();
        let view = store
            .save(AiSettingsInput {
                settings: AiSettings {
                    enabled: true,
                    provider: AiProvider::Xcli,
                    base_url: String::new(),
                    chat_model: String::new(),
                    embed_model: String::new(),
                    vector_provider: Some(AiProvider::Bailian),
                    vector_base_url: String::new(),
                    auto_scan_commitments: true,
                    ..AiSettings::default()
                },
                api_key: None,
                vector_api_key: Some("synthetic-vector-key".into()),
            })
            .unwrap();
        assert_eq!(view.settings.base_url, "xcli");
        assert_eq!(view.settings.chat_model, "codex");
        assert_eq!(view.settings.vector_base_url, BAILIAN_BASE_URL);
        assert_eq!(view.settings.embed_model, BAILIAN_EMBED_MODEL);
        assert!(!view.text_is_local);
        assert!(!view.vectors_is_local);
        assert!(!view.settings.allow_cloud);
        assert!(!view.settings.auto_scan_commitments);
        assert!(view.has_vector_api_key);
        assert!(!serde_json::to_string(&view)
            .unwrap()
            .contains("synthetic-vector-key"));
        assert_eq!(store.api_key().as_deref(), Some("synthetic-text-key"));
        assert_eq!(
            store.vector_api_key().as_deref(),
            Some("synthetic-vector-key")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.dir.join("vector-api-key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let mut input = AiSettingsInput {
            settings: view.settings,
            api_key: None,
            vector_api_key: None,
        };
        assert!(store.save(input.clone()).unwrap().has_vector_api_key);
        input.vector_api_key = Some(String::new());
        assert!(!store.save(input.clone()).unwrap().has_vector_api_key);
        assert_eq!(store.vector_api_key(), None);
        assert_eq!(store.api_key().as_deref(), Some("synthetic-text-key"));

        input.settings.vector_provider = Some(AiProvider::Xcli);
        assert!(store.save(input).is_err());
        assert_eq!(store.load().vector_provider, Some(AiProvider::Bailian));
    }

    #[test]
    fn bailian_keeps_namespace_endpoint_and_requires_its_own_key() {
        let temporary = tempdir().unwrap();
        let store = SettingsStore::new(temporary.path().join("ai"));
        store
            .save(AiSettingsInput {
                settings: AiSettings::default(),
                api_key: Some("old-provider-canary".into()),
                vector_api_key: None,
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
            ..AiSettings::default()
        };
        assert!(store
            .save(AiSettingsInput {
                settings: bailian.clone(),
                api_key: None,
                vector_api_key: None,
            })
            .is_err());
        let view = store
            .save(AiSettingsInput {
                settings: bailian,
                api_key: Some("sk-bailian".into()),
                vector_api_key: None,
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
                    ..AiSettings::default()
                },
                api_key: Some("sk-bailian".into()),
                vector_api_key: None,
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
                vector_api_key: None,
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
                vector_api_key: None,
            })
            .unwrap();
        assert!(local.settings.auto_scan_commitments);
        store
            .save(AiSettingsInput {
                settings: view.settings.clone(),
                api_key: None,
                vector_api_key: None,
            })
            .unwrap();
        assert_eq!(view.host, "api.example.com");
        assert_eq!(store.api_key().as_deref(), Some("sk-test"));

        let kept = store
            .save(AiSettingsInput {
                settings: store.load(),
                api_key: None,
                vector_api_key: None,
            })
            .unwrap();
        assert!(kept.has_api_key);
        let cleared = store
            .save(AiSettingsInput {
                settings: store.load(),
                api_key: Some(String::new()),
                vector_api_key: None,
            })
            .unwrap();
        assert!(!cleared.has_api_key);
    }
}
