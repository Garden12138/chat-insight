use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::models::{
    BootstrapResult, NormalizedMessage, PlatformCapabilities, SyncProgress, SyncResult,
};
use crate::portable::normalize_message_kind;
use crate::wechat_account::{self, AccountIdentity, AccountStore};
use chrono::{DateTime, Datelike, Timelike};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const PAGE_SIZE: usize = 500;

#[derive(Debug, Clone)]
pub struct BridgePaths {
    pub directory: PathBuf,
    pub cli: PathBuf,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    pub wxkey: PathBuf,
}

impl BridgePaths {
    pub fn resolve(app: &AppHandle) -> Self {
        let directory = std::env::var_os("CHAT_INSIGHT_BRIDGE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                app.path()
                    .resource_dir()
                    .ok()
                    .map(|path| path.join("wechat-bridge"))
            })
            .unwrap_or_else(|| PathBuf::from("resources/wechat-bridge"));
        #[cfg(target_os = "windows")]
        let cli = directory.join("wechat-cli.exe");
        #[cfg(not(target_os = "windows"))]
        let cli = directory.join("wechat-cli");
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let wxkey = directory.join("wxkey");
        Self {
            directory,
            cli,
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            wxkey,
        }
    }
}

pub fn capabilities(app: &AppHandle) -> PlatformCapabilities {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let direct_wechat_read =
        (os == "macos" && arch == "aarch64") || (os == "windows" && arch == "x86_64");
    let paths = BridgePaths::resolve(app);
    let bridge_bundled = direct_wechat_read && paths.cli.is_file();
    let (setup_state, setup_hint) = if !direct_wechat_read {
        (
            "import_only",
            if os == "macos" && arch == "x86_64" {
                "参考数据桥暂未提供 macOS Intel 版本，请导入由 Chat Insight 导出的 JSONL 文件。"
            } else {
                "当前平台暂不支持直接读取微信，请导入 Chat Insight JSONL 文件。"
            },
        )
    } else if !bridge_bundled {
        (
            "bridge_missing",
            "安装包中的微信数据桥缺失，请重新安装完整版本。",
        )
    } else {
        (
            "needs_setup",
            if os == "macos" {
                "需要为数据桥授予完全磁盘访问权限；wxkey 引导可能退出并重新打开微信，并显示系统管理员授权窗口。"
            } else {
                "请保持微信 4.x 已登录，并至少打开一个普通聊天以便完成本机密钥初始化。"
            },
        )
    };
    PlatformCapabilities {
        os: os.into(),
        arch: arch.into(),
        direct_wechat_read,
        bridge_bundled,
        setup_state: setup_state.into(),
        setup_hint: setup_hint.into(),
    }
}

/// Points the bridge at the WeChat account currently in use. With `require_switch`,
/// an unchanged account is reported back instead of re-running setup.
pub fn connect(
    app: &AppHandle,
    store: &AccountStore,
    require_switch: bool,
) -> AppResult<BootstrapResult> {
    let current = capabilities(app);
    if !current.direct_wechat_read {
        return Err(AppError::Bridge("当前平台只支持导入分析".into()));
    }
    let paths = BridgePaths::resolve(app);
    if !paths.cli.is_file() {
        return Err(AppError::Bridge(
            "安装包中的微信数据桥缺失，请重新安装完整版本".into(),
        ));
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if !paths.wxkey.is_file() {
        return Err(AppError::Bridge("安装包中的 wxkey 组件缺失".into()));
    }

    let config_path = wechat_account::bridge_config_path()?;
    let previous_config = wechat_account::read_optional(&config_path)?;
    let previous = previous_config
        .as_deref()
        .and_then(wechat_account::identity_from_config_bytes);
    let active = wechat_account::detect_active_account();

    if let (Some(config), Some(from), Some(to)) = (&previous_config, &previous, &active) {
        if !from.same_account(to) {
            switch_account(app, &paths, store, &config_path, config, from, to)?;
            return Ok(BootstrapResult {
                ready: true,
                message: "已切换到当前登录的微信账号".into(),
                switched: true,
                account_id: Some(to.account_id()),
            });
        }
    }
    if require_switch {
        if active.is_none() {
            return Err(AppError::Bridge(
                "没有找到正在使用的微信账号，请确认微信已登录并点开任意一个聊天后重试".into(),
            ));
        }
        if let Some(from) = &previous {
            return Ok(BootstrapResult {
                ready: true,
                message: "微信当前登录的账号没有变化".into(),
                switched: false,
                account_id: Some(from.account_id()),
            });
        }
    }

    let target = active.as_ref().or(previous.as_ref());
    run_platform_bootstrap(app, &paths, target.and_then(|item| item.db_root.as_deref()))?;
    emit_progress(app, "complete", 1, 1, "本机微信读取已准备完成", 0, false);
    Ok(BootstrapResult {
        ready: true,
        message: "本机微信读取已准备完成".into(),
        switched: previous.is_none(),
        account_id: target.map(AccountIdentity::account_id),
    })
}

fn switch_account(
    app: &AppHandle,
    paths: &BridgePaths,
    store: &AccountStore,
    config_path: &Path,
    previous_config: &[u8],
    from: &AccountIdentity,
    to: &AccountIdentity,
) -> AppResult<()> {
    store.save(from, previous_config)?;
    if let Some(saved) = store.load(to)? {
        emit_progress(
            app,
            "preparing",
            0,
            1,
            "正在切换到之前连接过的微信账号…",
            0,
            true,
        );
        wechat_account::write_private_file(config_path, &saved)?;
        emit_progress(app, "complete", 1, 1, "已切换微信账号", 0, false);
        return Ok(());
    }
    // wxkey skips key extraction while any ready config exists.
    wechat_account::remove_if_exists(config_path)?;
    if let Err(error) = run_platform_bootstrap(app, paths, to.db_root.as_deref()) {
        let _ = wechat_account::write_private_file(config_path, previous_config);
        return Err(error);
    }
    emit_progress(app, "complete", 1, 1, "已切换微信账号", 0, false);
    Ok(())
}

fn run_platform_bootstrap(
    app: &AppHandle,
    paths: &BridgePaths,
    db_root: Option<&Path>,
) -> AppResult<()> {
    emit_progress(app, "preparing", 0, 1, "正在启动本机授权引导…", 0, true);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    run_wxkey_bootstrap(app, paths, db_root)?;
    #[cfg(target_os = "windows")]
    run_status(
        &paths.cli,
        &paths.directory,
        &["cache", "refresh", "--force"],
        db_root,
    )?;
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        target_os = "windows"
    )))]
    let _ = (paths, db_root);
    Ok(())
}

pub fn sync_wechat(
    app: &AppHandle,
    database: &Database,
    cancel: &AtomicBool,
) -> AppResult<SyncResult> {
    let paths = BridgePaths::resolve(app);
    if !paths.cli.is_file() {
        return Err(AppError::Bridge(
            "安装包中的微信数据桥缺失，请重新安装完整版本".into(),
        ));
    }
    emit_progress(app, "preparing", 0, 0, "正在检查本机微信数据…", 0, true);
    let status = run_json(&paths, &["status", "--include-debug"])?;
    let identity = wechat_account::identity_from_status(&status).ok_or_else(|| {
        AppError::Bridge("无法识别当前连接的微信账号，请重新连接本机微信后再同步".into())
    })?;
    let account_id = identity.account_id();
    let account_name = self_display_name(&paths, identity.self_wxid())
        .unwrap_or_else(|| identity.self_wxid().to_string());
    database.upsert_account(&account_id, &account_name, std::env::consts::OS)?;
    let run_id = database.start_sync_run(&account_id)?;

    let sessions = match read_all_sessions(&paths, app, cancel) {
        Ok(items) => items,
        Err(error) => {
            database.finish_sync_run(run_id, "error", 0, 0, 1)?;
            return Err(error);
        }
    };
    let total = sessions.len();
    let mut chats_processed = 0usize;
    let mut messages_imported = 0usize;
    let mut warnings = 0usize;

    for (index, session) in sessions.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            database.finish_sync_run(
                run_id,
                "cancelled",
                chats_processed,
                messages_imported,
                warnings,
            )?;
            emit_progress(
                app,
                "cancelled",
                index,
                total,
                "同步已取消，已完成的数据已经保存",
                warnings,
                false,
            );
            return Ok(SyncResult {
                account_id,
                chats_processed,
                messages_imported,
                warnings,
                cancelled: true,
            });
        }
        database.upsert_chat(
            &account_id,
            &session.chat_id,
            &session.display_name,
            &session.chat_kind,
        )?;
        emit_progress(
            app,
            "messages",
            index,
            total,
            &format!("正在同步第 {} 个聊天…", index + 1),
            warnings,
            true,
        );
        match sync_chat(&paths, database, &account_id, &session.chat_id, cancel) {
            Ok(imported) => messages_imported += imported,
            Err(_) => warnings += 1,
        }
        chats_processed += 1;
    }

    let status = if warnings == 0 {
        "complete"
    } else {
        "complete_with_warnings"
    };
    database.finish_sync_run(run_id, status, chats_processed, messages_imported, warnings)?;
    emit_progress(
        app,
        "complete",
        total,
        total,
        if warnings == 0 {
            "同步完成"
        } else {
            "同步完成，部分聊天可稍后重试"
        },
        warnings,
        false,
    );
    Ok(SyncResult {
        account_id,
        chats_processed,
        messages_imported,
        warnings,
        cancelled: false,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    chat_id: String,
    display_name: String,
    chat_kind: String,
}

fn read_all_sessions(
    paths: &BridgePaths,
    app: &AppHandle,
    cancel: &AtomicBool,
) -> AppResult<Vec<Session>> {
    let mut result = Vec::new();
    let mut seen_chats = HashSet::new();
    let mut seen_cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut args = vec![
            "sessions".to_string(),
            "--limit".into(),
            PAGE_SIZE.to_string(),
        ];
        if let Some(value) = &cursor {
            args.extend(["--cursor".into(), value.clone()]);
        }
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let value = run_json(paths, &refs)?;
        let page = parse_sessions(&value);
        for session in page {
            if seen_chats.insert(session.chat_id.clone()) {
                result.push(session);
            }
        }
        emit_progress(
            app,
            "sessions",
            result.len(),
            0,
            "正在整理聊天列表…",
            0,
            true,
        );
        let has_more = find_bool_by_keys(&value, &["has_more", "hasMore"]).unwrap_or(false);
        let next = value
            .pointer("/data/query/cursor/next")
            .and_then(value_to_string)
            .or_else(|| {
                value
                    .pointer("/data/query/next_cursor")
                    .and_then(value_to_string)
            })
            .or_else(|| find_string_by_keys(&value, &["next_cursor", "nextCursor", "cursor"]));
        if !has_more || next.as_deref().map_or(true, str::is_empty) {
            break;
        }
        let next = next.expect("cursor checked above");
        if !seen_cursors.insert(next.clone()) {
            break;
        }
        cursor = Some(next);
    }
    Ok(result)
}

fn sync_chat(
    paths: &BridgePaths,
    database: &Database,
    account_id: &str,
    chat_id: &str,
    cancel: &AtomicBool,
) -> AppResult<usize> {
    let existing_max = database.max_local_id(account_id, chat_id)?.unwrap_or(0);
    let mut before: Option<i64> = None;
    let mut imported = 0usize;
    let mut newest_seen = existing_max;
    let mut seen_anchors = HashSet::new();

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut args = vec![
            "timeline".to_string(),
            chat_id.to_string(),
            "--limit".into(),
            PAGE_SIZE.to_string(),
        ];
        if let Some(anchor) = before {
            args.extend(["--before-message".into(), anchor.to_string()]);
        }
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let value = run_json(paths, &refs)?;
        let messages = parse_messages(&value, chat_id);
        if messages.is_empty() {
            break;
        }
        let minimum = messages
            .iter()
            .map(|message| message.local_id)
            .min()
            .unwrap_or(0);
        let maximum = messages
            .iter()
            .map(|message| message.local_id)
            .max()
            .unwrap_or(0);
        newest_seen = newest_seen.max(maximum);
        for message in &messages {
            if database.insert_message(account_id, message)? {
                imported += 1;
            }
        }
        if (existing_max > 0 && minimum <= existing_max)
            || messages.len() < PAGE_SIZE
            || minimum == 0
            || !seen_anchors.insert(minimum)
        {
            break;
        }
        before = Some(minimum);
    }
    database.save_cursor(account_id, chat_id, newest_seen)?;
    Ok(imported)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run_wxkey_bootstrap(
    app: &AppHandle,
    paths: &BridgePaths,
    db_root: Option<&Path>,
) -> AppResult<()> {
    use std::process::Stdio;
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::Duration;

    emit_progress(
        app,
        "preparing",
        0,
        1,
        "正在启动本机授权引导；如出现“wechat-cli setup”密码窗口，请输入这台 Mac 的登录密码…",
        0,
        true,
    );
    let bridge_exited = |_| AppError::Bridge("本机微信数据桥意外退出".into());
    let mut command = Command::new(&paths.wxkey);
    command.current_dir(&paths.directory).arg("bootstrap");
    if let Some(root) = db_root {
        command.arg("--root").arg(root);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| AppError::Bridge("无法启动本机微信数据桥".into()))?;
    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        forward_lines(stdout, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        forward_lines(stderr, sender.clone());
    }
    drop(sender);

    let mut last_error = None;
    let mut handle_line = |line: String| {
        if let Some(message) = wxkey_output::stage_message(&line) {
            emit_progress(app, "preparing", 0, 1, message, 0, true);
        }
        if let Some(detail) = wxkey_output::error_detail(&line) {
            last_error = Some(detail.to_string());
        }
    };
    // Processes started by wxkey may inherit the pipes, so exit is detected with try_wait
    // instead of waiting for both streams to close.
    let status = loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => handle_line(line),
            Err(RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait().map_err(bridge_exited)? {
                    break status;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break child.wait().map_err(bridge_exited)?,
        }
    };
    while let Ok(line) = receiver.recv_timeout(Duration::from_millis(200)) {
        handle_line(line);
    }
    if !status.success() {
        return Err(AppError::Bridge(wxkey_output::failure_message(
            last_error.as_deref(),
        )));
    }
    Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn forward_lines(
    stream: impl std::io::Read + Send + 'static,
    sender: std::sync::mpsc::Sender<String>,
) {
    use std::io::{BufRead, BufReader};

    std::thread::spawn(move || {
        for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
            if sender
                .send(String::from_utf8_lossy(&line).into_owned())
                .is_err()
            {
                break;
            }
        }
    });
}

#[cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
mod wxkey_output {
    const ERROR_PREFIX: &str = "[wxkey] ERROR:";

    const STAGES: &[(&str, &str)] = &[
        (
            "=== wxkey bootstrap ===",
            "管理员授权已确认，正在检查微信…",
        ),
        (
            "Preparing wechat-cli WeChat shadow copy",
            "正在复制并签名一份微信副本，可能需要几分钟…",
        ),
        (
            "Opening wechat-cli WeChat shadow copy",
            "正在打开微信副本，请在新打开的微信里登录，并点开任意一个聊天…",
        ),
        (
            "[INFO] Opening WeChat...",
            "正在打开微信，请登录并点开任意一个聊天…",
        ),
        (
            "Extracting keys and writing config",
            "正在读取微信数据库密钥，请保持微信登录并点开任意一个聊天…",
        ),
        (
            "PBKDF fallback: launching",
            "正在重新启动微信读取密钥（最长约 5 分钟），请在新打开的微信里登录，并点开任意一个聊天…",
        ),
        ("Extracting image_key", "正在读取图片解码密钥…"),
    ];

    pub fn stage_message(line: &str) -> Option<&'static str> {
        STAGES
            .iter()
            .find(|(marker, _)| line.contains(marker))
            .map(|(_, message)| *message)
    }

    pub fn error_detail(line: &str) -> Option<&str> {
        line.split_once(ERROR_PREFIX)
            .map(|(_, detail)| detail.trim())
            .filter(|detail| !detail.is_empty())
    }

    pub fn failure_message(detail: Option<&str>) -> String {
        let Some(detail) = detail else {
            return "微信数据初始化未完成，请确认授权后重试".into();
        };
        let hint = if detail.contains("WeChat app not found") {
            "未找到微信应用，请确认微信安装在“应用程序”文件夹中后重试"
        } else if detail.contains("sudo password") {
            "管理员授权未完成，请重试并在“wechat-cli setup”窗口中输入这台 Mac 的登录密码（需要管理员账户）"
        } else if detail.contains("lldb") {
            "读取新版微信密钥需要 Xcode 命令行工具，请在“终端”运行 xcode-select --install 安装后重试"
        } else if detail.contains("no WeChat 4.x account directory") {
            "未找到微信数据目录，请登录微信并收发一条消息后重试"
        } else if detail.contains("not ready yet")
            || detail.contains("did not start")
            || detail.contains("no keys")
        {
            "没有读取到微信密钥，请保持微信登录并点开任意一个聊天后重试"
        } else {
            return format!("微信数据初始化未完成：{detail}");
        };
        hint.into()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn maps_bootstrap_output_to_user_facing_stages() {
            assert_eq!(
                stage_message("[INFO] Preparing wechat-cli WeChat shadow copy..."),
                Some("正在复制并签名一份微信副本，可能需要几分钟…")
            );
            assert_eq!(
                stage_message("[INFO] Opening wechat-cli WeChat shadow copy..."),
                Some("正在打开微信副本，请在新打开的微信里登录，并点开任意一个聊天…")
            );
            assert_eq!(
                stage_message("[INFO] Opening WeChat..."),
                Some("正在打开微信，请登录并点开任意一个聊天…")
            );
            assert_eq!(
                stage_message("[wxkey] scanned 12 MB / 3 regions, found=0"),
                None
            );
        }

        #[test]
        fn extracts_the_error_detail_from_wxkey_output() {
            assert_eq!(
                error_detail(
                    "[wxkey] ERROR: WeChat app not found at /Applications/WeChat.app: no such file"
                ),
                Some("WeChat app not found at /Applications/WeChat.app: no such file")
            );
            assert_eq!(error_detail("[INFO] Opening WeChat..."), None);
            assert_eq!(error_detail("[wxkey] ERROR:   "), None);
        }

        #[test]
        fn explains_known_wxkey_failures() {
            assert!(failure_message(Some(
                "WeChat app not found at /Applications/WeChat.app: stat: no such file or directory"
            ))
            .starts_with("未找到微信应用"));
            assert!(failure_message(Some(
                "prepare stored sudo credential: sudo password prompt cancelled or failed: exit status 1"
            ))
            .starts_with("管理员授权未完成"));
            assert!(
                failure_message(Some("lldb python path unavailable: exit status 1"))
                    .contains("xcode-select --install")
            );
            assert_eq!(
                failure_message(Some("write config: permission denied")),
                "微信数据初始化未完成：write config: permission denied"
            );
            assert_eq!(
                failure_message(None),
                "微信数据初始化未完成，请确认授权后重试"
            );
        }
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn run_status(
    binary: &Path,
    directory: &Path,
    args: &[&str],
    db_root: Option<&Path>,
) -> AppResult<()> {
    let mut command = Command::new(binary);
    command.current_dir(directory).args(args);
    // wechat-cli refuses to pick between several account directories on its own.
    if let Some(root) = db_root {
        command.env("WECHAT_CLI_DB_ROOT", root);
    }
    let status = command
        .status()
        .map_err(|_| AppError::Bridge("无法启动本机微信数据桥".into()))?;
    if !status.success() {
        return Err(AppError::Bridge(
            "微信数据初始化未完成，请确认授权后重试".into(),
        ));
    }
    Ok(())
}

fn run_json(paths: &BridgePaths, args: &[&str]) -> AppResult<Value> {
    let output = Command::new(&paths.cli)
        .current_dir(&paths.directory)
        .args(args)
        .output()
        .map_err(|_| AppError::Bridge("无法启动本机微信数据桥".into()))?;
    if !output.status.success() {
        return Err(AppError::Bridge(
            "微信本地数据读取失败，请确认微信已登录并完成初始化".into(),
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| AppError::Bridge("微信数据桥返回了无法识别的结果".into()))?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(AppError::Bridge(
            "微信数据桥报告读取失败，请检查应用内的准备步骤".into(),
        ));
    }
    Ok(value)
}

fn self_display_name(paths: &BridgePaths, self_wxid: &str) -> Option<String> {
    let value = run_json(
        paths,
        &["contacts", "--keyword", self_wxid, "--limit", "20"],
    )
    .ok()?;
    find_contact_nickname(&value, self_wxid)
}

fn find_contact_nickname(value: &Value, username: &str) -> Option<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_contact_nickname(item, username)),
        Value::Object(object) => {
            if object.get("username").and_then(Value::as_str) == Some(username) {
                return ["nick_name", "display_name"]
                    .iter()
                    .filter_map(|key| object.get(*key).and_then(Value::as_str))
                    .map(str::trim)
                    .find(|name| !name.is_empty() && *name != username)
                    .map(str::to_string);
            }
            object
                .values()
                .find_map(|child| find_contact_nickname(child, username))
        }
        _ => None,
    }
}

fn parse_sessions(value: &Value) -> Vec<Session> {
    find_array_by_keys(value, &["sessions", "chats", "items", "results"])
        .into_iter()
        .filter_map(|item| {
            let chat_id = find_string_by_keys(&item, &["talker", "chat_id", "chatId", "username"])?;
            let display_name = find_string_by_keys(
                &item,
                &["display_name", "displayName", "remark", "nickname", "name"],
            )
            .unwrap_or_else(|| "未命名聊天".into());
            let chat_kind = if chat_id.ends_with("@chatroom")
                || find_string_by_keys(&item, &["chat_kind", "chatKind", "type"]).as_deref()
                    == Some("group")
            {
                "group"
            } else {
                "direct"
            };
            Some(Session {
                chat_id,
                display_name,
                chat_kind: chat_kind.into(),
            })
        })
        .collect()
}

fn parse_messages(value: &Value, fallback_chat_id: &str) -> Vec<NormalizedMessage> {
    find_array_by_keys(value, &["messages", "items", "results", "timeline"])
        .into_iter()
        .filter_map(|item| normalize_message(&item, fallback_chat_id))
        .collect()
}

fn normalize_message(value: &Value, fallback_chat_id: &str) -> Option<NormalizedMessage> {
    let local_id = value
        .pointer("/id/local_id")
        .and_then(Value::as_i64)
        .or_else(|| find_i64_by_keys(value, &["local_id", "localId"]))?;
    let server_id = value
        .pointer("/id/server_id_str")
        .and_then(value_to_string)
        .or_else(|| find_string_by_keys(value, &["server_id_str", "serverId", "server_id"]));
    let chat_id = value
        .pointer("/id/talker")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| find_string_by_keys(value, &["talker", "chat_id", "chatId"]))
        .unwrap_or_else(|| fallback_chat_id.to_string());
    let occurred_at =
        find_string_by_keys(value, &["time_iso", "timeIso", "occurred_at", "occurredAt"]).or_else(
            || {
                find_i64_by_keys(value, &["timestamp", "create_time", "createTime"])
                    .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
                    .map(|date| date.to_rfc3339())
            },
        )?;
    let parsed = DateTime::parse_from_rfc3339(&occurred_at).ok()?;
    let sender_name = find_string_by_keys(
        value,
        &[
            "sender",
            "sender_name",
            "senderName",
            "sender_contact_display",
        ],
    )
    .unwrap_or_default();
    let sender_id =
        find_string_by_keys(value, &["sender_wxid", "sender_id", "senderId"]).unwrap_or_default();
    let is_from_me = find_bool_by_keys(value, &["is_from_me", "isFromMe"]).unwrap_or(false);
    let kind = normalize_message_kind(
        &find_string_by_keys(value, &["kind", "type", "message_type"])
            .unwrap_or_else(|| "unknown".into()),
    );
    let text = find_string_by_keys(value, &["text", "content", "summary"]).unwrap_or_default();
    Some(NormalizedMessage {
        chat_id,
        local_id,
        server_id,
        occurred_at,
        local_date: parsed.format("%Y-%m-%d").to_string(),
        local_hour: parsed.hour(),
        local_weekday: parsed.weekday().num_days_from_monday(),
        sender_id,
        sender_name,
        is_from_me,
        kind,
        text,
    })
}

fn find_array_by_keys(value: &Value, keys: &[&str]) -> Vec<Value> {
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(items) = object.get(*key).and_then(Value::as_array) {
                return items.clone();
            }
        }
        for child in object.values() {
            let found = find_array_by_keys(child, keys);
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

fn find_string_by_keys(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(found) = object.get(*key).and_then(value_to_string) {
                if !found.is_empty() {
                    return Some(found);
                }
            }
        }
        for child in object.values() {
            if let Some(found) = find_string_by_keys(child, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn find_i64_by_keys(value: &Value, keys: &[&str]) -> Option<i64> {
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(found) = object
                .get(*key)
                .and_then(|item| item.as_i64().or_else(|| item.as_str()?.parse().ok()))
            {
                return Some(found);
            }
        }
        for child in object.values() {
            if let Some(found) = find_i64_by_keys(child, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn find_bool_by_keys(value: &Value, keys: &[&str]) -> Option<bool> {
    if let Some(object) = value.as_object() {
        for key in keys {
            if let Some(found) = object.get(*key).and_then(Value::as_bool) {
                return Some(found);
            }
        }
        for child in object.values() {
            if let Some(found) = find_bool_by_keys(child, keys) {
                return Some(found);
            }
        }
    }
    None
}

fn value_to_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.as_i64().map(|item| item.to_string()))
}

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    current: usize,
    total: usize,
    message: &str,
    warnings: usize,
    running: bool,
) {
    let _ = app.emit(
        "sync-progress",
        SyncProgress {
            phase: phase.into(),
            current,
            total,
            message: message.into(),
            warnings,
            running,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upstream_envelope_without_logging_content() {
        let value: Value = serde_json::from_str(include_str!("../testdata/timeline.json")).unwrap();
        let messages = parse_messages(&value, "fallback");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].local_id, 42);
        assert_eq!(messages[0].chat_id, "team@chatroom");
        assert_eq!(messages[0].local_weekday, 5);
        assert_eq!(messages[0].local_hour, 20);
    }

    #[test]
    fn finds_own_nickname_among_fuzzy_contact_matches() {
        let value = serde_json::json!({
            "ok": true,
            "data": [
                { "username": "wxid_abc123x", "nick_name": "别人" },
                { "username": "wxid_abc123", "nick_name": " 小曾 ", "display_name": "小曾" }
            ]
        });
        assert_eq!(
            find_contact_nickname(&value, "wxid_abc123").as_deref(),
            Some("小曾")
        );
        let unnamed = serde_json::json!({ "data": [{ "username": "wxid_a", "nick_name": "" , "display_name": "wxid_a" }] });
        assert_eq!(find_contact_nickname(&unnamed, "wxid_a"), None);
    }

    #[test]
    fn parses_sessions_and_detects_groups() {
        let value: Value = serde_json::from_str(include_str!("../testdata/sessions.json")).unwrap();
        let sessions = parse_sessions(&value);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[1].chat_kind, "group");
    }
}
