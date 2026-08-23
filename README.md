# Chat Insight

Chat Insight 是一个本地优先的微信聊天可视化桌面应用。它把微信本机数据解析成应用自己的
SQLite 快照，并提供消息趋势、活跃时段、聊天排行、消息类型、关键词和单聊天时间线。

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
Release 构建使用版本化资产，并核验 GitHub Release 元数据发布的 SHA-256；不会跟随 `latest`。

## 隐私边界

- 聊天正文、统计和关键词仅保存在本机应用数据目录。
- 应用不会上传数据、发送消息、控制微信界面或启动实时 watcher。
- 不复制或修改微信原始数据库；直接读取由内置数据桥完成，应用只写自己的 SQLite。
- 日志不记录聊天正文、数据库密钥、原始数据库路径或联系人标识。
- JSONL 导出包含聊天正文，请像保护聊天记录一样保管导出文件。

macOS Apple Silicon 首次初始化会运行上游 `wxkey bootstrap`。它可能要求完全磁盘访问权限、
退出并重新打开微信，并显示系统管理员授权窗口。界面会在执行前明确提示这些行为。

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
