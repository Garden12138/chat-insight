#[cfg(all(test, target_os = "macos"))]
mod acceptance;
#[cfg(target_os = "macos")]
mod isolation;

use super::client::{cancelled, ChatMessage, LlmClient, OpenAiCompatClient};
use crate::error::{AppError, AppResult};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::ffi::OsStr;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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

pub fn build_xcli_request(messages: &[ChatMessage], agent: &str) -> Value {
    let goal = messages
        .iter()
        .map(|message| {
            let prefix = match message.role {
                "system" => "【要求】\n",
                "assistant" => "【之前的回复】\n",
                _ => "",
            };
            format!("{prefix}{}", message.content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut request = json!({
        "version": 1,
        "capability": "ask",
        "goal": goal,
        "timeout_sec": TASK_TIMEOUT_SECONDS,
        "metadata": {"app": "chat-insight"}
    });
    let agent = normalize_xcli_agent(agent);
    if agent != "auto" {
        request["agent"] = json!(agent);
    }
    request
}

const TASK_TIMEOUT_SECONDS: u64 = 600;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(100);
const AGENTS_TIMEOUT: Duration = Duration::from_secs(15);
const STDERR_BYTES: usize = 4096;
pub const NO_VECTORS: &str = "xcli:no-vectors";

pub fn vectors_unconfigured() -> AppError {
    AppError::Ai("未配置向量服务，请在 AI 设置中为 xcli 选择向量服务".into())
}

#[derive(Debug, Deserialize)]
pub struct XcliAgentStatus {
    pub name: String,
    pub available: bool,
}

enum XcliInvocation {
    #[cfg(all(test, unix))]
    Fake(PathBuf),
    #[cfg(target_os = "macos")]
    Isolated(isolation::IsolatedXcli),
}

impl XcliInvocation {
    fn command(&self) -> Command {
        match *self {
            #[cfg(all(test, unix))]
            Self::Fake(ref executable) => {
                let mut command = Command::new(executable);
                command
                    .env_remove("FAKE_XCLI_MODE")
                    .env_remove("FAKE_XCLI_STDIN");
                command
            }
            #[cfg(target_os = "macos")]
            Self::Isolated(ref isolation) => isolation.command(),
        }
    }
}

fn missing_xcli(executable: &Path) -> AppError {
    AppError::Ai(format!(
        "未找到 xcli（{}），请从 https://github.com/Garden12138/xcli/releases 安装，或在 AI 设置中填写 xcli 的绝对路径",
        executable.display()
    ))
}

pub struct XcliClient {
    executable: AppResult<PathBuf>,
    agent: &'static str,
    pub(super) vectors: Option<OpenAiCompatClient>,
    #[cfg(all(test, unix))]
    fake_executable: bool,
}

impl XcliClient {
    pub fn new(
        executable: AppResult<PathBuf>,
        agent: &str,
        vectors: Option<OpenAiCompatClient>,
    ) -> Self {
        Self {
            executable,
            agent: normalize_xcli_agent(agent),
            vectors,
            #[cfg(all(test, unix))]
            fake_executable: false,
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn with_fake_executable(executable: PathBuf, agent: &str) -> Self {
        Self {
            fake_executable: true,
            ..Self::new(Ok(executable), agent, None)
        }
    }

    fn executable(&self) -> AppResult<&Path> {
        self.executable
            .as_deref()
            .map_err(|error| AppError::Validation(error.to_string()))
    }

    fn invocation(&self) -> AppResult<XcliInvocation> {
        if cfg!(windows) {
            return Err(AppError::Ai("xcli 暂不支持 Windows".into()));
        }
        let executable = self.executable()?;
        #[cfg(all(test, unix))]
        if self.fake_executable {
            return Ok(XcliInvocation::Fake(executable.to_path_buf()));
        }
        #[cfg(target_os = "macos")]
        {
            isolation::IsolatedXcli::prepare(executable, self.agent).map(XcliInvocation::Isolated)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = executable;
            Err(AppError::Ai("当前平台尚未启用 xcli 的读取隔离".into()))
        }
    }

    fn spawn(&self, command: &mut Command) -> AppResult<Child> {
        let executable = self.executable()?;
        command.spawn().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                missing_xcli(executable)
            } else {
                AppError::Ai(format!(
                    "无法启动 xcli（{}）：{error}",
                    executable.display()
                ))
            }
        })
    }

    pub fn agents(&self) -> AppResult<Vec<XcliAgentStatus>> {
        let invocation = self.invocation()?;
        let mut command = invocation.command();
        let mut child = self.spawn(
            command
                .args(["agents", "--json"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let output = thread::spawn(move || {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output)?;
            Ok::<_, io::Error>(output)
        });
        let stderr = read_stderr(child.stderr.take().expect("piped stderr"));
        let deadline = Instant::now() + AGENTS_TIMEOUT;
        let Some(status) = wait_for_exit(&mut child, deadline)? else {
            force_exit(&mut child);
            return Err(AppError::Ai("xcli CLI 状态检查超时".into()));
        };
        if !status.success() {
            return Err(exit_error(status, stderr));
        }
        let bytes = output
            .join()
            .map_err(|_| AppError::Ai("无法读取 xcli CLI 状态".into()))??;
        serde_json::from_slice(&bytes)
            .map_err(|_| AppError::Ai("xcli 返回的 CLI 状态格式无效".into()))
    }
}

impl LlmClient for XcliClient {
    fn chat(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> AppResult<String> {
        let invocation = self.invocation()?;
        let mut command = invocation.command();
        let request = serde_json::to_vec(&build_xcli_request(messages, self.agent))?;
        let mut child = self.spawn(
            command
                .args(["submit", "--request", "-", "--events"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )?;
        let events = read_events(child.stdout.take().expect("piped stdout"));
        let stderr = read_stderr(child.stderr.take().expect("piped stderr"));
        // Read the process result even if xcli closes stdin before accepting the request.
        let _ = child.stdin.take().expect("piped stdin").write_all(&request);
        let mut task = String::new();
        let mut result = None;
        loop {
            let event = match events.recv_timeout(CANCEL_POLL_INTERVAL) {
                Ok(line) => match serde_json::from_slice::<Value>(&line) {
                    Ok(event) if event["type"].is_string() => Some(event),
                    _ => continue,
                },
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if !on_delta("") {
                drop(events);
                terminate(&mut child);
                return Err(cancelled());
            }
            let Some(event) = event else { continue };
            if task.is_empty() {
                task = event["task"].as_str().unwrap_or_default().to_owned();
            }
            if event["type"] == "task.finished" {
                result = Some(parse_result(&event["data"], &task));
                break;
            }
        }
        drop(events);
        let Some(status) = wait_for_exit(&mut child, Instant::now() + SHUTDOWN_TIMEOUT)? else {
            terminate(&mut child);
            return Err(AppError::Ai("xcli 在结果流结束后未退出".into()));
        };
        match result {
            Some(Ok(summary)) => {
                if !on_delta(&summary) {
                    return Err(cancelled());
                }
                Ok(summary)
            }
            Some(Err(error)) => Err(error),
            None => Err(exit_error(status, stderr)),
        }
    }

    fn embed(&self, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        self.vectors
            .as_ref()
            .ok_or_else(vectors_unconfigured)?
            .embed(inputs)
    }

    fn embed_model(&self) -> &str {
        self.vectors
            .as_ref()
            .map_or(NO_VECTORS, LlmClient::embed_model)
    }

    fn validate_embed_model(&self) -> AppResult<()> {
        self.vectors
            .as_ref()
            .map_or(Ok(()), LlmClient::validate_embed_model)
    }
}

fn parse_result(data: &Value, task: &str) -> AppResult<String> {
    if data["status"] == "cancelled" {
        return Err(cancelled());
    }
    if data["status"] == "completed" {
        return data["summary"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| AppError::Ai(format!("xcli 未返回生成文本（任务 {task}）")));
    }
    let task = data["task_id"].as_str().unwrap_or(task);
    let kind = data["error"]["kind"]
        .as_str()
        .or_else(|| data["status"].as_str())
        .unwrap_or("unknown");
    let message = data["error"]["message"]
        .as_str()
        .unwrap_or("未返回错误详情");
    Err(AppError::Ai(format!(
        "xcli 生成失败（{kind}）：{message}（任务 {task}）"
    )))
}

fn wait_for_exit(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn terminate(child: &mut Child) {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    #[cfg(unix)]
    if matches!(child.try_wait(), Ok(None)) {
        // sandbox-exec replaces itself with xcli. xcli handles SIGTERM and reaps its CLI.
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    if !matches!(wait_for_exit(child, deadline), Ok(Some(_))) {
        force_exit(child);
    }
}

fn force_exit(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_events(stdout: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (sender, events) = mpsc::sync_channel(16);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::new();
        while reader
            .read_until(b'\n', &mut line)
            .is_ok_and(|count| count > 0)
        {
            if sender.send(std::mem::take(&mut line)).is_err() {
                // Let xcli write its final event while the caller waits for shutdown.
                let _ = io::copy(&mut reader, &mut io::sink());
                break;
            }
        }
    });
    events
}

fn read_stderr(mut stderr: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut tail = Vec::with_capacity(STDERR_BYTES * 2);
        let mut buffer = [0; STDERR_BYTES];
        while let Ok(count) = stderr.read(&mut buffer) {
            if count == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..count]);
            if tail.len() > STDERR_BYTES {
                tail.drain(..tail.len() - STDERR_BYTES);
            }
        }
        tail
    })
}

fn exit_error(status: ExitStatus, stderr: JoinHandle<Vec<u8>>) -> AppError {
    let bytes = stderr.join().unwrap_or_default();
    let decoded = String::from_utf8_lossy(&bytes);
    let tail = decoded.trim();
    let offset = tail
        .char_indices()
        .rev()
        .nth(499)
        .map_or(0, |(offset, _)| offset);
    let code = status
        .code()
        .map_or_else(|| status.to_string(), |code| code.to_string());
    AppError::Ai(format!(
        "xcli 异常退出（退出码 {code}）：{}",
        &tail[offset..]
    ))
}

#[cfg(all(test, unix))]
pub(super) mod tests {
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
    pub(crate) fn make_fake_xcli(dir: &Path, mode: &str) -> PathBuf {
        let path = dir.join("xcli");
        fs::write(dir.join("mode"), mode).unwrap();
        fs::write(
            &path,
            r##"#!/bin/sh
set -eu
dir=${0%/*}
FAKE_XCLI_MODE=${FAKE_XCLI_MODE:-$(cat "$dir/mode")}
FAKE_XCLI_STDIN=${FAKE_XCLI_STDIN:-"$dir/stdin.json"}
if [ "$*" = "agents --json" ]; then
    printf '%s\n' '[{"name":"codex","available":true},{"name":"cursor","available":false}]'
    exit 0
fi
[ "$*" = "submit --request - --events" ]
cat > "$FAKE_XCLI_STDIN"
case "$FAKE_XCLI_MODE" in
    ok)
        printf '%s\n' 'ignored non-JSON line' \
            '{"task":"t-fake","type":"task.started","data":{}}' \
            '{"task":"t-fake","type":"step.progress","data":{"text":"synthetic progress"}}' \
            '{"task":"t-fake","type":"task.finished","data":{"task_id":"t-fake","status":"completed","summary":"合成摘要"}}'
        ;;
    fail)
        printf '%s\n' '{"task":"t-fake","type":"task.finished","data":{"task_id":"t-fake","status":"failed","error":{"kind":"quota","message":"codex 额度暂不可用"}}}'
        exit 1
        ;;
    noresult)
        printf '%s\n' 'boom' >&2
        exit 3
        ;;
    hang)
        on_term() {
            printf '%s\n' 'SIGTERM' > "$dir/signal.txt"
            printf '%s\n' '{"task":"t-fake","type":"task.finished","data":{"task_id":"t-fake","status":"cancelled"}}' |
                tee "$dir/terminal.json"
            exit 130
        }
        trap on_term TERM
        printf '%s\n' 'ready' > "$dir/ready"
        printf '%s\n' '{"task":"t-fake","type":"task.started","data":{}}'
        # No events arrive during generation. End the fixture even when the test regresses.
        for cycle in 1 2 3 4; do sleep 1; done
        exit 3
        ;;
    *) exit 2 ;;
esac
"##,
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[test]
    fn request_joins_messages_and_only_pins_an_explicit_agent() {
        let messages = [
            ChatMessage::system("合成要求"),
            ChatMessage::user("合成问题"),
            ChatMessage {
                role: "assistant",
                content: "合成前文".into(),
            },
            ChatMessage::user("合成追问"),
        ];
        let request = build_xcli_request(&messages, "auto");
        assert_eq!(
            request,
            json!({
                "version": 1,
                "capability": "ask",
                "goal": "【要求】\n合成要求\n\n合成问题\n\n【之前的回复】\n合成前文\n\n合成追问",
                "timeout_sec": 600,
                "metadata": {"app": "chat-insight"}
            })
        );
        assert_eq!(build_xcli_request(&messages, "codex")["agent"], "codex");
    }

    #[test]
    fn refuses_to_send_chat_when_the_launcher_has_no_read_isolation() {
        let dir = tempdir().unwrap();
        let client = XcliClient::new(Ok(make_fake_xcli(dir.path(), "ok")), "cursor", None);
        let error = client
            .chat(&[ChatMessage::user("合成问题")], &mut |_| true)
            .unwrap_err();
        assert!(error.to_string().contains("读取隔离"));
        assert!(!dir.path().join("stdin.json").exists());
    }

    #[test]
    fn sends_request_on_stdin_and_returns_only_the_final_text() {
        let dir = tempdir().unwrap();
        let client = XcliClient::with_fake_executable(make_fake_xcli(dir.path(), "ok"), "codex");
        let messages = [ChatMessage::user("合成问题")];
        let mut deltas = Vec::new();
        let summary = client
            .chat_limited(&messages, 16, &mut |delta| {
                deltas.push(delta.to_string());
                true
            })
            .unwrap();
        assert_eq!(summary, "合成摘要");
        assert_eq!(deltas.last().map(String::as_str), Some("合成摘要"));
        assert!(deltas.len() >= 4);
        assert!(deltas[..deltas.len() - 1].iter().all(String::is_empty));
        let request = fs::read(dir.path().join("stdin.json")).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&request).unwrap(),
            build_xcli_request(&messages, "codex")
        );
    }

    #[test]
    fn handles_failed_and_unfinished_tasks() {
        for (mode, expected) in [
            ("fail", ["quota", "t-fake"]),
            ("noresult", ["退出码 3", "boom"]),
        ] {
            let dir = tempdir().unwrap();
            let client = XcliClient::with_fake_executable(make_fake_xcli(dir.path(), mode), "auto");
            let error = client.chat(&[], &mut |_| true).unwrap_err().to_string();
            for fragment in expected {
                assert!(error.contains(fragment), "{error}");
            }
        }
    }

    #[test]
    fn sends_sigterm_even_while_xcli_has_no_events() {
        let dir = tempdir().unwrap();
        let client = XcliClient::with_fake_executable(make_fake_xcli(dir.path(), "hang"), "auto");
        let ready = dir.path().join("ready");
        let mut saw_ready = false;
        let error = client
            .chat(&[], &mut |_| {
                if !ready.is_file() {
                    return true;
                }
                !std::mem::replace(&mut saw_ready, true)
            })
            .unwrap_err();
        assert_eq!(error.to_string(), cancelled().to_string());
        assert_eq!(
            fs::read_to_string(dir.path().join("signal.txt")).unwrap(),
            "SIGTERM\n"
        );
        let terminal: Value =
            serde_json::from_slice(&fs::read(dir.path().join("terminal.json")).unwrap()).unwrap();
        assert_eq!(terminal["data"]["status"], "cancelled");
    }

    #[test]
    fn lists_agents_and_reports_unconfigured_vectors() {
        let dir = tempdir().unwrap();
        let client = XcliClient::with_fake_executable(make_fake_xcli(dir.path(), "ok"), "auto");
        let agents = client.agents().unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "codex");
        assert!(agents[0].available);
        assert_eq!(agents[1].name, "cursor");
        assert!(!agents[1].available);
        assert_eq!(client.embed_model(), "xcli:no-vectors");
        assert!(client.validate_embed_model().is_ok());
        let error = client.embed(&[]).unwrap_err().to_string();
        assert!(error.contains("未配置向量服务"));
    }
}
