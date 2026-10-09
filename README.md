# Chat Insight

Chat Insight 是一个本地优先的微信聊天可视化桌面应用。它把微信本机数据解析成应用自己的
SQLite 快照，并提供消息趋势、活跃时段、聊天排行、消息类型、关键词和单聊天时间线。
时间线中的图片、视频和表情包可直接预览，点击图片或视频后全屏查看或播放。

可选的 AI 功能（默认关闭）提供：消息全文搜索与带出处的自然语言问答、单个聊天的摘要、按语义归纳的
“常聊话题”，以及年度报告与关系洞察。AI 通过 OpenAI 兼容接口调用模型，默认连接本机的
[Ollama](https://ollama.com)（`qwen3:8b` 对话、`bge-m3` 向量），也可以在设置中换成其他服务。
关键词搜索、年度统计和关系洞察不依赖 AI。

![Status](https://img.shields.io/badge/status-MVP-fa6c4c)
![Tauri](https://img.shields.io/badge/Tauri-2-245b57)
![License](https://img.shields.io/badge/license-MIT-e6a34b)

## 平台能力

| 平台 | 安装包 | 直接读取微信 | 数据导入分析 |
| --- | --- | --- | --- |
| macOS Apple Silicon | DMG | 微信 4.x，内置固定版本数据桥 | 支持 |
| macOS Intel | DMG | 暂不支持 | 支持 |
| Windows x64 | NSIS 安装器 | 微信 4.x，内置固定版本数据桥 | 支持 |

微信数据桥固定为 [`r266-tech/wechat-cli v1.6.20`](https://github.com/r266-tech/wechat-cli)。
由于上游 GitHub Release 当前不可访问，Release 在原生 runner 上从 Go 模块代理缓存的固定
`v1.6.20` 源码构建数据桥，并从腾讯官方固定 `WCDB v2.1.15` 源码构建数据库运行库；所有
源码归档均校验仓库内固定的 SHA-256，不会跟随 `latest`。上游 `wxkey v1.4.8` 只认
`/Applications/WeChat.app`，构建时会应用 `scripts/patches/` 中的补丁，让它同时识别
`/Applications/微信.app`。

## 隐私边界

- 聊天正文、统计和关键词仅保存在本机应用数据目录。
- 应用不会发送消息、控制微信界面或启动实时 watcher；除非你开启云端 AI 并逐次确认，否则不会上传数据
  （见下方“AI 功能”）。
- 不复制或修改微信原始数据库；直接读取由内置数据桥完成，应用只写自己的 SQLite。
- 日志不记录聊天正文、数据库密钥、原始数据库路径或联系人标识。
- JSONL 导出包含聊天正文，请像保护聊天记录一样保管导出文件。
- 图片和视频不会复制进应用数据库。查看时由内置数据桥按消息定位本机文件；微信加密图片会由数据桥
  解码到 `~/.wechat-cli/media-cache`（同步时数据桥也会写入这里）。界面只能按消息请求媒体，
  应用只提供微信账号目录和该缓存目录内的图片、视频文件。
- 表情包只存在于微信 CDN，本机缓存是加密的。显示表情时，应用会从微信 CDN（仅限 `qq.com`、
  `qpic.cn` 域名）下载该表情图片，并按 MD5 缓存到应用数据目录的 `sticker-cache/`，之后不再联网。
  下载请求只包含表情地址，不上传任何聊天内容。除你配置的 AI 模型服务外，这是应用自身唯一会发起的
  网络请求。
- 只有当前连接的微信账号能预览媒体；导入的数据集或其他账号的数据集会显示占位。微信已清理的媒体、
  尚未下载的视频需要先在微信中打开一次。

### AI 功能

- AI 默认关闭，需要在“数据管理 → AI 设置”中手动开启。设置保存在应用数据目录的 `ai/`，API Key
  单独存放且仅当前用户可读。
- 服务地址是 `localhost`、`127.0.0.1` 或 `::1` 时视为本机模型（如 Ollama），请求不会离开这台电脑。
- 其他地址视为云端服务，必须先勾选“允许使用云端模型服务”。凡是会发送聊天内容的请求，每次都会先
  弹窗说明发送到哪个主机、发送多少内容，确认后才发送；后端也会拒绝没有确认标记的请求。开启云端后，
  侧栏的“仅在本机”标识会变为“已启用云端 AI”。
- 各功能发送给模型的内容：
  - 语义索引：把聊天按对话分段（约 12 条消息或 600 字一段），发送给向量模型；向量只保存在本机数据库。
  - 问答：你的问题，以及检索到的最多 12 段相关聊天片段。
  - 聊天摘要：所选聊天在当前日期范围内的文本消息（较长时分段发送）。
  - 常聊话题：该聊天尚未索引的消息（用于向量），以及每个话题最多 5 段代表性片段。
  - 年度报告：只发送统计数字和聊天对象名称，不发送任何聊天内容；使用云端服务时默认用“联系人1号”
    这样的代号替换名称，生成后在本机还原。
- 摘要、话题和年度报告的结果缓存在本机数据库，数据有变化时自动失效。日志不记录提示词、回答或 API Key。

macOS Apple Silicon 首次初始化会运行上游 `wxkey bootstrap`。它可能要求完全磁盘访问权限、
退出并重新打开微信，并显示系统管理员授权窗口。界面会在执行前明确提示这些行为。

每个微信账号的记录分别保存为独立的数据集。换号后，先在微信里登录新账号并点开任意一个聊天，
再到“数据管理 → 切换微信账号”。应用会按微信最近写入的账号目录识别当前账号；新账号需要重新
授权一次。切换前，当前账号的数据桥配置（含数据库密钥）会备份到应用数据目录的
`wechat-accounts/`，文件仅当前用户可读，因此切回已连接过的账号时不需要重新授权。

## 本地开发

前置要求：Node.js 22+、Rust stable，以及
[Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
npm install
npm run dev          # 仅浏览器界面预览
npm run tauri dev    # 完整桌面应用
npm test
npm run lint
cargo test --manifest-path src-tauri/Cargo.toml
```

开发环境默认不包含微信数据桥。在受支持平台调试直读功能时：

```bash
CHAT_INSIGHT_TARGET=aarch64-apple-darwin npm run bridge:fetch
# 或 CHAT_INSIGHT_TARGET=x86_64-pc-windows-msvc npm run bridge:fetch
```

也可以设置 `CHAT_INSIGHT_BRIDGE_DIR` 指向已经准备好的完整上游包目录。不要提交该目录、微信
数据库、密钥配置、导出文件或真实聊天 fixture。

## 便携数据格式

`chat-insight-jsonl` schema v1 的首行是 manifest，后续记录为 `chat` 或 `message`。导入使用
单个 SQLite 事务；未知 schema、损坏记录或缺少聊天声明时整次回滚。重复导入相同文件不会
产生重复消息。导出不包含密钥、微信数据库、媒体文件或绝对媒体路径。

## 发布

普通 push/PR 运行 TypeScript、React 和 Rust 检查。推送与应用版本一致的 `v*` 标签后，
GitHub Actions 会在原生 runner 上构建并发布：

- `Chat-Insight_<version>_macOS_aarch64.dmg`
- `Chat-Insight_<version>_macOS_x64.dmg`
- `Chat-Insight_<version>_Windows_x64-setup.exe`
- `SHA256SUMS`

MVP 暂不接入正式代码签名。macOS 使用 ad-hoc 签名且未公证；首次打开时可能需要在 Finder
中右键选择“打开”，或在“系统设置 → 隐私与安全性”中确认。Windows SmartScreen 可能显示
“未知发布者”，请核对 GitHub Release 中的 SHA-256 后再选择继续运行。

## License

Chat Insight 使用 [MIT License](LICENSE)。第三方组件说明见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
