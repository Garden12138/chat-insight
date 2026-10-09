# 媒体浏览与界面焕新 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在聊天时间线内联预览图片与视频，并把界面与应用图标焕新为“C · 暖色现代风”。

**Architecture:** Rust 侧新增 `media.rs`，按 `chat_id + local_id` 调用内置数据桥 `wechat-cli media` 获取本机媒体路径，经白名单校验后通过自定义协议 `chatmedia` 提供给网页（支持 Range）。前端新增 `MediaBubble` 与 `Lightbox`，`ChatDetail` 按消息类型渲染。视觉焕新集中在 `styles.css` 设计令牌、`BrandMark` 组件与图表配色。

**Tech Stack:** Tauri 2.11（Rust）、React 19、TypeScript、ECharts 6、Vitest + Testing Library。

## Global Constraints

- 所有新增/修改文件 UTF-8 编码。
- 不改数据库 schema；不复制或修改微信原始数据；JSONL 导出不含媒体。
- 日志与错误信息不包含媒体路径、文件名或哈希。
- 网页只能按消息身份请求媒体，不能传入任意文件路径。
- 视觉：奶油底 `#faf6ef`、珊瑚橙渐变 `#ff9a76 → #f1442a`、圆角 14–20px、柔和投影；深色模式同步适配。
- 验证命令：`npm run lint`、`npm test`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`。

## File Structure

| 文件 | 职责 |
| --- | --- |
| `src-tauri/src/media.rs`（新） | 解析 `media` 输出、路径白名单、Range 解析、解析结果缓存、协议响应 |
| `src-tauri/src/bridge.rs` | 暴露 `run_json` 与 `BridgePaths` 供 `media.rs` 复用 |
| `src-tauri/src/wechat_account.rs` | 暴露当前桥账号识别（读取配置） |
| `src-tauri/src/models.rs` | `MediaInfo` |
| `src-tauri/src/lib.rs` | 注册 `media_info` 命令与 `chatmedia` 协议 |
| `src-tauri/tauri.conf.json` | CSP 加入协议 |
| `src/types.ts`、`src/api.ts`、`src/media.ts`（新） | `MediaInfo` 类型、`api.mediaInfo`、`mediaUrl` |
| `src/components/MediaBubble.tsx`（新） | 懒加载缩略图/封面与状态占位 |
| `src/components/Lightbox.tsx`（新） | 全屏查看、键盘导航 |
| `src/components/BrandMark.tsx`（新） | 新品牌标识 SVG |
| `src/components/Icons.tsx` | 新增 `play`、`close`、`chevronLeft`、`image` |
| `src/views/*.tsx`、`src/App.tsx`、`src/styles.css` | 视觉焕新 |
| `src/theme.ts`（新） | 图表配色常量 |
| `src-tauri/icons/*` | 新图标与重新生成的各尺寸文件 |

---

### Task 1: `media.rs` 纯函数（解析、白名单、Range）

**Files:** Create `src-tauri/src/media.rs`；Modify `src-tauri/src/lib.rs`（`mod media;`）

**Produces:**
- `pub struct MediaPaths { pub image: Option<PathBuf>, pub video: Option<PathBuf>, pub poster: Option<PathBuf> }`
- `pub fn parse_media(value: &serde_json::Value) -> MediaPaths`
- `pub fn is_allowed(path: &Path, roots: &[PathBuf]) -> bool`（存在、白名单扩展名、规范化后位于某个 root 内）
- `pub fn parse_range(header: &str, len: u64) -> Option<(u64, u64)>`（闭区间）
- `pub fn content_type(path: &Path) -> &'static str`

- [ ] 写测试：图片消息（`images[].path`）解析为 `image`；已下载视频（`.mp4` + `_thumb.jpg`）解析为 `video` + `poster`；仅封面视频只有 `poster`；root 外与非白名单扩展名被拒绝；`bytes=0-`、`bytes=10-19`、`bytes=-5`、越界 Range。
- [ ] 运行 `cargo test media` 确认失败，实现，再确认通过，提交。

### Task 2: 解析器、命令与 `chatmedia` 协议

**Files:** Modify `media.rs`、`bridge.rs`、`wechat_account.rs`、`models.rs`、`lib.rs`、`tauri.conf.json`

**Consumes:** Task 1 全部函数。

**Produces:**
- `pub struct MediaResolver`（`Clone`，内部 `Arc<Mutex<HashMap<(String, String, i64), MediaPaths>>>`），`fn resolve(&self, app, account_id, chat_id, local_id) -> AppResult<MediaPaths>`
- 命令 `media_info(account_id: String, chat_id: String, local_id: i64) -> MediaInfo { image: bool, video: bool, poster: bool, reason: Option<String> }`
- 协议路径 `/<variant>/<account>/<chat>/<local_id>`（各段 percent-encoded），`variant ∈ {image, video, poster}`；返回 200/206/404/416。

- [ ] 写测试：URL 路径解析（含百分号编码的 `@chatroom`）；账号不匹配返回拒绝原因。
- [ ] 实现、`cargo test`、`cargo clippy` 无新增告警，提交。

### Task 3: 前端媒体组件

**Files:** Create `src/media.ts`、`src/components/MediaBubble.tsx`、`src/components/Lightbox.tsx`、对应 `*.test.tsx`；Modify `src/types.ts`、`src/api.ts`、`src/views/ChatDetail.tsx`、`src/components/Icons.tsx`

**Consumes:** `media_info` 命令与协议 URL 格式。

**Produces:**
- `mediaUrl(variant: "image" | "video" | "poster", accountId: string, chatId: string, localId: number): string`
- `<MediaBubble accountId chatId message onOpen={(info) => void} />`
- `<Lightbox items={MediaItem[]} index onIndexChange onClose />`，`MediaItem = { message: MessageItem; info: MediaInfo }`

- [ ] 写测试：`mediaUrl` 编码；`MediaBubble` 加载中、可用（渲染图片/播放按钮）、不可用（显示原因）；`Lightbox` 的 Esc 关闭、左右键切换。
- [ ] 实现、`npm test`、`npm run lint`，提交。

### Task 4: 视觉焕新

**Files:** Modify `src/styles.css`、`src/App.tsx`、`src/views/*.tsx`、`src/components/Chart.tsx`（如需）；Create `src/components/BrandMark.tsx`、`src/theme.ts`

- [ ] 重写设计令牌与全部组件样式（侧栏浅暖色、选中项橙色淡底、卡片大圆角柔和投影、深色模式）。
- [ ] 用 `BrandMark` 替换三处 `.brand-mark` span 结构；图表配色改为 `theme.ts` 常量。
- [ ] `npm test`、`npm run lint`、`npm run build`，在 `npm run dev` 下截图检查，提交。

### Task 5: 应用图标与文档

**Files:** `src-tauri/icons/app-icon.svg` 及生成文件；`README.md`

- [ ] 写入新 SVG，`npx tauri icon src-tauri/icons/app-icon.svg` 重新生成，检查 32px 清晰度。
- [ ] README 增加图片/视频浏览说明与隐私说明（数据桥图片解码缓存位置）。提交。

### Task 6: 全量验证

- [ ] `npm run lint && npm test && npm run build && cargo test --manifest-path src-tauri/Cargo.toml` 全部通过。
- [ ] `npm run tauri dev`（如环境允许）检查真实图片与视频。
