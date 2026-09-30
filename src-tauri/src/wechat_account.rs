use crate::database::Database;
use crate::error::{AppError, AppResult};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Dataset id used before accounts were identified by their WeChat directory.
pub const LEGACY_ACCOUNT_ID: &str = "wechat:wechat-local";

const ACTIVITY_SCAN_DEPTH: usize = 3;
const NON_ACCOUNT_DIRS: &[&str] = &[
    "all_users",
    "applet",
    "backup",
    "wmpf",
    "app_data",
    "xwechat_files",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    pub dir_name: String,
    pub db_root: Option<PathBuf>,
}

impl AccountIdentity {
    pub fn from_parts(wxid: Option<&str>, db_root: Option<&str>) -> Option<Self> {
        let db_root = db_root
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let dir_name = db_root
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .or_else(|| {
                wxid.map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })?;
        Some(Self { dir_name, db_root })
    }

    pub fn from_root(root: &Path) -> Option<Self> {
        Self::from_parts(None, root.to_str())
    }

    pub fn account_id(&self) -> String {
        format!("wechat:{}", self.dir_name)
    }

    /// WeChat 4.x account directories append a 4-character suffix to the wxid.
    pub fn self_wxid(&self) -> &str {
        match self.dir_name.rsplit_once('_') {
            Some((wxid, suffix)) if !wxid.is_empty() && suffix.len() == 4 => wxid,
            _ => &self.dir_name,
        }
    }

    pub fn same_account(&self, other: &Self) -> bool {
        match (&self.db_root, &other.db_root) {
            (Some(left), Some(right)) => same_path(left, right),
            _ => self.dir_name == other.dir_name,
        }
    }
}

pub fn identity_from_config(value: &Value) -> Option<AccountIdentity> {
    AccountIdentity::from_parts(
        value.get("wxid").and_then(Value::as_str),
        value.get("db_root").and_then(Value::as_str),
    )
}

pub fn identity_from_config_bytes(bytes: &[u8]) -> Option<AccountIdentity> {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| identity_from_config(&value))
}

/// Reads the account from `wechat-cli status --include-debug`.
pub fn identity_from_status(value: &Value) -> Option<AccountIdentity> {
    let account = value.pointer("/data/status/account_debug")?;
    identity_from_config(account)
}

/// Mirrors wechat-cli's config lookup so the app swaps the file the bridge reads.
pub fn bridge_config_path() -> AppResult<PathBuf> {
    for name in ["WECHAT_CLI_CONFIG", "WX_MCP_CONFIG"] {
        if let Some(value) = std::env::var_os(name) {
            let value = value.to_string_lossy().trim().to_string();
            if !value.is_empty() {
                return Ok(PathBuf::from(value));
            }
        }
    }
    let home = directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| AppError::Bridge("无法定位当前用户的主目录".into()))?;
    Ok(home.join(".config").join("wxcli").join("config.json"))
}

pub fn read_optional(path: &Path) -> AppResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn remove_if_exists(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The bridge config holds database keys, so it is only ever written owner-readable.
pub fn write_private_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Bridge("微信数据桥配置路径无效".into()))?;
    create_private_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Bridge("微信数据桥配置路径无效".into()))?;
    let temporary = parent.join(format!(
        ".{file_name}.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| -> AppResult<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn create_private_dir(path: &Path) -> AppResult<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

/// Keeps each account's bridge config so switching back does not need a new key scan.
#[derive(Debug, Clone)]
pub struct AccountStore {
    directory: PathBuf,
}

impl AccountStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn save(&self, identity: &AccountIdentity, config: &[u8]) -> AppResult<()> {
        let Some(path) = self.path_for(identity) else {
            return Ok(());
        };
        write_private_file(&path, config)
    }

    pub fn load(&self, identity: &AccountIdentity) -> AppResult<Option<Vec<u8>>> {
        let Some(path) = self.path_for(identity) else {
            return Ok(None);
        };
        let Some(bytes) = read_optional(&path)? else {
            return Ok(None);
        };
        let matches =
            identity_from_config_bytes(&bytes).is_some_and(|saved| saved.same_account(identity));
        Ok(matches.then_some(bytes))
    }

    fn path_for(&self, identity: &AccountIdentity) -> Option<PathBuf> {
        let name = identity.dir_name.as_str();
        let safe = !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '@'));
        safe.then(|| self.directory.join(format!("{name}.json")))
    }
}

/// Moves pre-identity data to the account the bridge was configured for at the time.
pub fn adopt_legacy_dataset(database: &Database) -> AppResult<bool> {
    let Some(config) = read_optional(&bridge_config_path()?)? else {
        return Ok(false);
    };
    let Some(identity) = identity_from_config_bytes(&config) else {
        return Ok(false);
    };
    database.adopt_legacy_account(
        LEGACY_ACCOUNT_ID,
        &identity.account_id(),
        identity.self_wxid(),
    )
}

/// Picks the account WeChat wrote to most recently; the logged-in account keeps
/// touching its database files while others stay idle.
pub fn detect_active_account() -> Option<AccountIdentity> {
    most_recent_account(&wechat_data_roots()).and_then(|root| AccountIdentity::from_root(&root))
}

fn most_recent_account(roots: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if NON_ACCOUNT_DIRS.contains(&name.as_str()) {
                continue;
            }
            let storage = entry.path().join("db_storage");
            if !storage.is_dir() {
                continue;
            }
            let Some(active_at) = latest_file_mtime(&storage, ACTIVITY_SCAN_DEPTH) else {
                continue;
            };
            if best.as_ref().map_or(true, |(time, _)| active_at > *time) {
                best = Some((active_at, entry.path()));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn latest_file_mtime(directory: &Path, depth: usize) -> Option<SystemTime> {
    let mut latest = None;
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let candidate = if kind.is_dir() {
            if depth == 0 {
                continue;
            }
            latest_file_mtime(&entry.path(), depth - 1)
        } else if kind.is_file() {
            entry.metadata().ok().and_then(|meta| meta.modified().ok())
        } else {
            None
        };
        latest = latest.max(candidate);
    }
    latest
}

fn wechat_data_roots() -> Vec<PathBuf> {
    let Some(dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    platform_data_roots(dirs.home_dir())
}

#[cfg(target_os = "macos")]
fn platform_data_roots(home: &Path) -> Vec<PathBuf> {
    let container = home
        .join("Library")
        .join("Containers")
        .join("com.tencent.xinWeChat")
        .join("Data");
    let mut roots = vec![container.join("Documents").join("xwechat_files")];
    let support = container
        .join("Library")
        .join("Application Support")
        .join("com.tencent.xinWeChat");
    if let Ok(entries) = fs::read_dir(support) {
        roots.extend(
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| entry.path()),
        );
    }
    roots
}

#[cfg(target_os = "windows")]
fn platform_data_roots(home: &Path) -> Vec<PathBuf> {
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.to_path_buf());
    let mut bases = vec![
        profile.join("Documents").join("xwechat_files"),
        profile.join("Documents").join("WeChat Files"),
        profile.join("WeChat Files"),
        profile
            .join("AppData")
            .join("Roaming")
            .join("Tencent")
            .join("WeChat")
            .join("WeChat Files"),
    ];
    if let Some(app_data) = std::env::var_os("APPDATA") {
        bases.push(
            PathBuf::from(app_data)
                .join("Tencent")
                .join("WeChat")
                .join("WeChat Files"),
        );
    }
    let mut roots = Vec::new();
    for base in bases {
        let nested = base.join("xwechat_files");
        for root in [base, nested] {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_data_roots(_home: &Path) -> Vec<PathBuf> {
    Vec::new()
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    fn touch(path: &Path, age: Duration) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = fs::File::create(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    #[test]
    fn identifies_accounts_by_directory_and_strips_wechat_suffix() {
        let identity = AccountIdentity::from_parts(
            Some("wxid_old"),
            Some("/data/xwechat_files/wxid_abc123_9741"),
        )
        .unwrap();
        assert_eq!(identity.account_id(), "wechat:wxid_abc123_9741");
        assert_eq!(identity.self_wxid(), "wxid_abc123");

        let custom = AccountIdentity::from_parts(Some("john_doe"), None).unwrap();
        assert_eq!(custom.self_wxid(), "john_doe");
        assert_eq!(AccountIdentity::from_parts(None, Some("  ")), None);
    }

    #[test]
    fn reads_identity_from_debug_status_envelope() {
        let value: Value = serde_json::json!({
            "ok": true,
            "data": {
                "identity": { "name": "wechat-cli" },
                "status": { "account_debug": { "wxid": "wxid_a_1234", "db_root": "/x/wxid_a_1234" } }
            }
        });
        let identity = identity_from_status(&value).unwrap();
        assert_eq!(identity.account_id(), "wechat:wxid_a_1234");
        assert!(identity_from_status(&serde_json::json!({ "data": { "status": {} } })).is_none());
    }

    #[test]
    fn detects_the_account_with_the_newest_database_activity() {
        let root = tempdir().unwrap();
        let old = root.path().join("wxid_old_aaaa");
        let new = root.path().join("wxid_new_bbbb");
        touch(
            &old.join("db_storage/message/message_0.db"),
            Duration::from_secs(3600),
        );
        touch(
            &new.join("db_storage/message/message_0.db-wal"),
            Duration::from_secs(5),
        );
        touch(
            &root.path().join("all_users/db_storage/login.db"),
            Duration::ZERO,
        );
        let detected = most_recent_account(&[root.path().to_path_buf()]).unwrap();
        assert_eq!(detected, new);
    }

    #[test]
    fn stores_configs_per_account_and_rejects_mismatched_files() {
        let directory = tempdir().unwrap();
        let store = AccountStore::new(directory.path());
        let account = AccountIdentity::from_parts(None, Some("/x/wxid_a_1234")).unwrap();
        let config = br#"{"wxid":"wxid_a_1234","db_root":"/x/wxid_a_1234","keys":{"s":"k"}}"#;
        store.save(&account, config).unwrap();
        assert_eq!(store.load(&account).unwrap().as_deref(), Some(&config[..]));

        let moved = AccountIdentity::from_parts(None, Some("/y/wxid_a_1234")).unwrap();
        assert_eq!(store.load(&moved).unwrap(), None);

        let unsafe_name = AccountIdentity::from_parts(Some("../escape"), None).unwrap();
        store.save(&unsafe_name, config).unwrap();
        assert_eq!(store.load(&unsafe_name).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_files_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempdir().unwrap();
        let path = directory.path().join("nested/config.json");
        write_private_file(&path, b"{}").unwrap();
        write_private_file(&path, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
