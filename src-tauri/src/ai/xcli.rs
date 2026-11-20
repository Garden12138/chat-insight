use crate::error::{AppError, AppResult};
use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const XCLI_AGENTS: [&str; 4] = ["auto", "codex", "antigravity", "cursor"];

pub fn normalize_xcli_agent(value: &str) -> &'static str {
    XCLI_AGENTS
        .into_iter()
        .find(|agent| *agent == value)
        .unwrap_or("auto")
}

pub fn resolve_xcli_executable(configured: &str) -> AppResult<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from);
    resolve_xcli_executable_with(
        configured,
        env::var("XCLI_PATH").ok().as_deref(),
        env::var_os("PATH").as_deref(),
        home.as_deref(),
    )
}

pub fn resolve_xcli_executable_with(
    configured: &str,
    xcli_path: Option<&str>,
    path_env: Option<&OsStr>,
    home: Option<&Path>,
) -> AppResult<PathBuf> {
    let configured = configured.trim();
    let path = if configured.contains('/') {
        expand_home(configured, home)
    } else {
        let named = (configured != "xcli" && !configured.is_empty())
            .then(|| find_in_path(configured, path_env))
            .flatten();
        named
            .or_else(|| {
                xcli_path
                    .map(|path| expand_home(path.trim(), home))
                    .filter(|path| path.is_file())
            })
            .or_else(|| find_in_path("xcli", path_env))
            .or_else(|| {
                home.map(|home| home.join(".local/bin/xcli"))
                    .filter(|path| path.is_file())
            })
            .unwrap_or_else(|| PathBuf::from("xcli"))
    };

    if path.file_name() != Some(OsStr::new("xcli")) {
        return Err(AppError::Validation(
            "xcli 可执行文件的文件名必须是 xcli".into(),
        ));
    }
    Ok(path)
}

fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~/"), home) {
        (Some(relative), Some(home)) => home.join(relative),
        _ => PathBuf::from(path),
    }
}

fn find_in_path(command: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    env::split_paths(path_env?)
        .filter(|directory| !directory.as_os_str().is_empty())
        .map(|directory| directory.join(command))
        .find(|path| path.is_file())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn fake_xcli(path: &Path) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        let fake = path.join("xcli");
        fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
        fake
    }

    #[test]
    fn resolves_xcli_executable_in_precedence_order() {
        let directory = tempdir().unwrap();
        let home = directory.path().join("home");
        let path_xcli = fake_xcli(&directory.path().join("path"));
        let pinned_xcli = fake_xcli(&directory.path().join("explicit"));
        let home_xcli = fake_xcli(&home.join(".local/bin"));
        let path_env = env::join_paths([path_xcli.parent().unwrap()]).unwrap();
        let xcli_path = pinned_xcli.to_str();

        let resolve = |configured, override_path, search_path| {
            resolve_xcli_executable_with(configured, override_path, search_path, Some(&home))
        };
        assert_eq!(
            resolve("~/tools/xcli", xcli_path, Some(&path_env)).unwrap(),
            home.join("tools/xcli")
        );
        assert_eq!(
            resolve("xcli", xcli_path, Some(&path_env)).unwrap(),
            pinned_xcli
        );
        assert_eq!(resolve("xcli", None, Some(&path_env)).unwrap(), path_xcli);
        assert_eq!(resolve("xcli", None, None).unwrap(), home_xcli);
        assert_eq!(
            resolve_xcli_executable_with("xcli", None, None, None).unwrap(),
            PathBuf::from("xcli")
        );
        assert_eq!(
            resolve("missing-command", xcli_path, Some(&path_env)).unwrap(),
            pinned_xcli
        );
        assert!(matches!(
            resolve("/bin/sh", None, None),
            Err(AppError::Validation(message)) if message == "xcli 可执行文件的文件名必须是 xcli"
        ));
    }

    #[test]
    fn normalizes_xcli_agent() {
        for agent in ["auto", "codex", "antigravity", "cursor"] {
            assert_eq!(normalize_xcli_agent(agent), agent);
        }
        assert_eq!(normalize_xcli_agent("xcli/text-default"), "auto");
    }
}
