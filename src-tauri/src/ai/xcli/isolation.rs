use super::find_in_path;
use crate::error::{AppError, AppResult};
use serde_json::json;
use std::env;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::{symlink, DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const SYSTEM_PATH: &str = "/usr/bin:/bin";
const SYSTEM_CA_BUNDLE: &str = "/private/etc/ssl/cert.pem";
const PROFILE: &str = r#"
(version 1)
(deny default)
(import "dyld-support.sb")
(allow process-fork)
(allow process-exec
    (literal (param "XCLI")) (literal (param "CODEX"))
    (literal "/usr/bin/git"))
(allow file-read* file-test-existence file-map-executable
    (subpath "/System/Library") (subpath "/usr/lib") (subpath "/usr/share")
    (literal (param "XCLI")) (literal (param "CODEX"))
    (literal "/usr/bin/git"))
(allow file-read* file-write* (subpath (param "RUN")))
(allow file-read* file-write*
    (literal (param "AUTH"))
    (literal (param "AUTH_LOCK"))
    (regex (string-append "^" (regex-quote (param "AUTH_DIR")) "/[.]auth-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]$")))
(allow file-read* file-write-data (literal "/dev/null") (literal "/dev/zero"))
(allow file-read-data
    (literal "/dev/urandom") (literal "/dev/random")
    (literal "/private/etc/ssl/cert.pem") (literal "/private/etc/ssl/openssl.cnf"))
(allow file-read-metadata file-test-existence)
(allow sysctl-read)
(allow ipc-posix-shm-read* (ipc-posix-name-prefix "apple.cfprefs."))
(allow process-info* signal (target same-sandbox))
(allow user-preference-read (preference-domain "com.openai.codex"))
(allow managed-preference-read (preference-domain "com.openai.codex"))
(allow mach-lookup
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon")
    (global-name "com.apple.mDNSResponder")
    (global-name "com.apple.dnssd.service")
    (global-name "com.apple.SystemConfiguration.configd")
    (global-name "com.apple.trustd.agent")
    (global-name "com.apple.trustd"))
(allow network-outbound
    (literal "/private/var/run/mDNSResponder")
    (require-all (remote tcp "*:443") (require-not (remote ip "localhost:*"))))
(unless (equal? (param "PROXY_ENDPOINT") "")
    (allow network-outbound (remote tcp (param "PROXY_ENDPOINT"))))
"#;

struct LocalProxy {
    host: &'static str,
    port: u16,
}

impl LocalProxy {
    fn parse(value: &str) -> Option<Self> {
        let authority = value.trim().strip_prefix("http://")?.trim_end_matches('/');
        let (host, port) = authority.rsplit_once(':')?;
        Self::new(host, port.parse().ok()?)
    }

    fn new(host: &str, port: u16) -> Option<Self> {
        let host = match host {
            "localhost" | "127.0.0.1" => "127.0.0.1",
            "::1" | "[::1]" => "[::1]",
            _ => return None,
        };
        (port != 0).then_some(Self { host, port })
    }

    fn detected() -> Option<Self> {
        ["HTTPS_PROXY", "https_proxy"]
            .into_iter()
            .find_map(|name| env::var(name).ok().and_then(|value| Self::parse(&value)))
            .or_else(Self::system)
    }

    fn system() -> Option<Self> {
        let output = Command::new("/usr/sbin/scutil")
            .arg("--proxy")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let output = String::from_utf8(output.stdout).ok()?;
        let setting = |wanted| {
            output.lines().find_map(|line| {
                let (name, value) = line.trim().split_once(" : ")?;
                (name == wanted).then_some(value)
            })
        };
        if setting("HTTPSEnable") != Some("1") {
            return None;
        }
        Self::new(setting("HTTPSProxy")?, setting("HTTPSPort")?.parse().ok()?)
    }
}

pub(super) struct IsolatedXcli {
    root: PathBuf,
    xcli: PathBuf,
    codex: PathBuf,
    auth: PathBuf,
    proxy: Option<LocalProxy>,
}

impl IsolatedXcli {
    pub(super) fn prepare(executable: &Path) -> AppResult<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| AppError::Ai("无法定位已登录 CLI 的主目录".into()))?;
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let codex = find_in_path("codex", env::var_os("PATH").as_deref())
            .or_else(|| {
                let local = home.join(".local/bin/codex");
                local.is_file().then_some(local)
            })
            .ok_or_else(|| AppError::Ai("未找到已安装的 Codex CLI".into()))?;
        let auth = codex_home.join("auth.json");
        let xcli_home = env::var_os("XCLI_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".xcli"));
        Self::prepare_in(
            executable,
            &codex,
            &auth,
            &xcli_home.join("chat-insight"),
            LocalProxy::detected(),
        )
    }

    fn prepare_in(
        executable: &Path,
        codex: &Path,
        auth: &Path,
        runs: &Path,
        proxy: Option<LocalProxy>,
    ) -> AppResult<Self> {
        let xcli = executable
            .canonicalize()
            .map_err(|_| super::missing_xcli(executable))?;
        let codex = codex
            .canonicalize()
            .map_err(|_| AppError::Ai("无法读取已安装的 Codex CLI".into()))?;
        let auth = auth.canonicalize().map_err(|_| codex_login_file_error())?;
        let metadata = auth.metadata().map_err(|_| codex_login_file_error())?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || auth.file_name() != Some("auth.json".as_ref())
        {
            return Err(codex_login_file_error());
        }
        // Check read access without reading or copying the credential.
        fs::File::open(&auth).map_err(|_| codex_login_file_error())?;
        DirBuilder::new().mode(0o700).recursive(true).create(runs)?;
        let root = runs
            .canonicalize()?
            .join(Uuid::new_v4().simple().to_string());
        DirBuilder::new().mode(0o700).create(&root)?;
        let isolated = Self {
            root,
            xcli,
            codex,
            auth,
            proxy,
        };
        for directory in ["xcli", "codex", "home", "tmp"] {
            DirBuilder::new()
                .mode(0o700)
                .create(isolated.root.join(directory))?;
        }
        // Codex follows this link when loading and rotating its existing credential.
        // No credential bytes are read or copied by Chat Insight.
        symlink(&isolated.auth, isolated.root.join("codex/auth.json"))?;
        let config = json!({
            "path": SYSTEM_PATH,
            "defaults": {"inherit_shell_proxy": false},
            "agents": {
                "codex": {
                    "command": isolated.codex,
                    "extra_args": [
                        "--ignore-user-config",
                        "--ignore-rules",
                        "--disable",
                        "shell_tool",
                        "-c",
                        "features.apps=false",
                        "-c",
                        "cli_auth_credentials_store=\"file\""
                    ],
                },
                "cursor": {"enabled": false, "command": "/dev/null"},
                "antigravity": {"enabled": false, "command": "/dev/null"},
            }
        });
        fs::write(
            isolated.root.join("xcli/config.yaml"),
            serde_json::to_vec(&config)?,
        )?;
        Ok(isolated)
    }

    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(SANDBOX_EXEC);
        let auth_lock = self.auth.with_extension("json.lock");
        command
            .env_clear()
            .env("PATH", SYSTEM_PATH)
            .env("LANG", "en_US.UTF-8")
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("CODEX_HOME", self.root.join("codex"))
            .env("XCLI_HOME", self.root.join("xcli"))
            .current_dir(&self.root);
        if Path::new(SYSTEM_CA_BUNDLE).is_file() {
            command.env("CODEX_CA_CERTIFICATE", SYSTEM_CA_BUNDLE);
        }
        let proxy_endpoint = self.proxy.as_ref().map_or_else(String::new, |proxy| {
            command.env(
                "HTTPS_PROXY",
                format!("http://{}:{}", proxy.host, proxy.port),
            );
            format!("localhost:{}", proxy.port)
        });
        command
            .arg("-D")
            .arg(format!("PROXY_ENDPOINT={proxy_endpoint}"));
        for (key, value) in [
            ("RUN", self.root.as_path()),
            ("XCLI", self.xcli.as_path()),
            ("CODEX", self.codex.as_path()),
            ("AUTH", self.auth.as_path()),
            ("AUTH_LOCK", auth_lock.as_path()),
            ("AUTH_DIR", self.auth.parent().expect("absolute auth path")),
        ] {
            let mut parameter = std::ffi::OsString::from(format!("{key}="));
            parameter.push(value);
            command.arg("-D").arg(parameter);
        }
        let profile = PROFILE.to_owned();
        #[cfg(test)]
        let profile =
            profile + "\n(allow process-exec file-read* file-map-executable (literal \"/bin/bash\") (literal \"/bin/mv\"))";
        command.args(["-p", &profile, "--"]).arg(&self.xcli);
        command
    }
}

fn codex_login_file_error() -> AppError {
    AppError::Ai(
        r#"需要 Codex 使用文件凭据存储。请在 ~/.codex/config.toml（设置了 CODEX_HOME 时为 $CODEX_HOME/config.toml）的顶层设置 cli_auth_credentials_store = "file"，运行 codex login，然后重试。登录文件 auth.json 必须是可读取且没有硬链接的普通文件。"#.into(),
    )
}

impl Drop for IsolatedXcli {
    fn drop(&mut self) {
        // Remove the link, leaving the original login and its renewal lock with Codex.
        let _ = fs::remove_file(self.root.join("codex/auth.json"));
        let has_task = fs::read_dir(self.root.join("xcli/tasks"))
            .ok()
            .is_some_and(|mut tasks| tasks.next().is_some());
        if !has_task {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn explains_file_login_for_missing_and_unsafe_credentials() {
        let fixture = tempdir().unwrap();
        let executable = fixture.path().join("xcli");
        fs::write(&executable, b"synthetic executable").unwrap();
        let auth = fixture.path().join("auth.json");
        let runs = fixture.path().join("runs");
        for create_directory in [false, true] {
            if create_directory {
                fs::create_dir(&auth).unwrap();
            }
            let error = IsolatedXcli::prepare_in(&executable, &executable, &auth, &runs, None)
                .err()
                .expect("invalid Codex login file should be rejected")
                .to_string();
            assert!(error.contains("~/.codex/config.toml"), "{error}");
            assert!(
                error.contains(r#"cli_auth_credentials_store = "file""#),
                "{error}"
            );
            assert!(error.contains("codex login"), "{error}");
            assert!(!runs.exists());
        }
    }

    #[test]
    fn allows_only_the_configured_proxy_to_receive_loopback_connections() {
        use std::net::TcpListener;

        let fixture = tempdir().unwrap();
        let auth = fixture.path().join("auth.json");
        fs::write(&auth, "synthetic login").unwrap();
        let allowed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let blocked = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let authorized = allowed.local_addr().unwrap().port();
        let unauthorized = blocked.local_addr().unwrap().port();
        let proxy = LocalProxy::parse(&format!("http://127.0.0.1:{authorized}")).unwrap();
        assert!(LocalProxy::parse("http://example.com:6738").is_none());
        assert!(LocalProxy::parse("http://127.0.0.1:6738/path").is_none());

        let xcli = fixture.path().join("xcli");
        fs::write(
            &xcli,
            r#"#!/bin/bash
set -eu
test "$HTTPS_PROXY" = "http://127.0.0.1:$1"
( : >/dev/tcp/127.0.0.1/"$1" ) || exit 11
if ( : >/dev/tcp/127.0.0.1/"$2" ) 2>/dev/null; then exit 12; fi
"#,
        )
        .unwrap();
        fs::set_permissions(&xcli, fs::Permissions::from_mode(0o700)).unwrap();
        let sandbox = IsolatedXcli::prepare_in(
            &xcli,
            &xcli,
            &auth,
            &fixture.path().join("runs"),
            Some(proxy),
        )
        .unwrap();
        let status = sandbox
            .command()
            .arg(authorized.to_string())
            .arg(unauthorized.to_string())
            .status()
            .unwrap();
        assert!(status.success(), "{status}");
    }

    #[test]
    fn subprocess_can_reuse_existing_login_but_cannot_read_previous_chats() {
        let fixture = tempdir().unwrap();
        let existing = fixture.path().join("existing");
        let runs = fixture.path().join("runs");
        fs::create_dir(&existing).unwrap();
        let auth = existing.join("auth.json");
        fs::write(&auth, "synthetic login\n").unwrap();
        let protected = existing.join("chat-insight.db");
        fs::write(&protected, "synthetic private\n").unwrap();
        let past = runs.join("previous");
        fs::create_dir_all(&past).unwrap();
        let old_session = past.join("session.jsonl");
        fs::write(&old_session, "synthetic previous task\n").unwrap();

        let xcli = fixture.path().join("xcli");
        fs::write(
            &xcli,
            r#"#!/bin/bash
set -eu
IFS= read -r auth < "$CODEX_HOME/auth.json"
[ "$auth" = 'synthetic login' ]
printf '%s\n' 'current synthetic task' > "$XCLI_HOME/current"
if (IFS= read -r private < "$1") 2>/dev/null; then exit 11; fi
/bin/bash -c 'if (IFS= read -r private < "$1") 2>/dev/null; then exit 12; fi' child "$2"
printf '' > "$3.lock"
printf '%s\n' 'synthetic renewed login' > "${3%/*}/.auth-check1"
/bin/mv "${3%/*}/.auth-check1" "$3"
"#,
        )
        .unwrap();
        fs::set_permissions(&xcli, fs::Permissions::from_mode(0o700)).unwrap();

        let sandbox = IsolatedXcli::prepare_in(&xcli, &xcli, &auth, &runs, None).unwrap();
        let run = sandbox.root.clone();
        let status = sandbox
            .command()
            .arg(&protected)
            .arg(&old_session)
            .arg(&sandbox.auth)
            .status()
            .unwrap();
        assert!(status.success(), "{status}");
        assert_eq!(
            fs::read_to_string(&auth).unwrap(),
            "synthetic renewed login\n"
        );
        assert_eq!(
            fs::read_to_string(run.join("xcli/current")).unwrap(),
            "current synthetic task\n"
        );
        drop(sandbox);
        assert!(!run.exists());
        assert!(old_session.is_file());
    }
}
