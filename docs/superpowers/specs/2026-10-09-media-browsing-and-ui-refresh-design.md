# 媒体浏览与界面焕新设计

日期：2026-10-09
状态：待用户审阅

## 目标

1. 在聊天时间线内联预览图片与视频，点击放大或播放。
2. 全面焕新桌面应用界面与应用图标，采用“C · 暖色现代风”。

## 已确认的决策

| 决策 | 结论 |
| --- | --- |
| 媒体浏览范围 | 仅聊天时间线内联预览，不做独立媒体库页 |
| 媒体来源 | 按需只读读取微信目录，不复制原始媒体；缩略图缓存到应用数据目录 |
| 媒体链路 | 应用自己定位并解码（方案 2），实施第一步先做真实数据摸底 |
| 视觉风格 | C · 暖色现代风：奶油白 + 珊瑚橙渐变，大圆角，柔和投影 |
| 焕新范围 | 全面焕新：侧栏、总览、聊天详情、数据管理、引导页、图表配色、图标 |

## 非目标

- 不做独立媒体库页、不做媒体搜索与筛选。
- 不复制或修改微信原始媒体与数据库。
- JSONL 导出仍不含媒体文件和绝对媒体路径。
- 不处理语音、文件、表情包等其他媒体类型。

## 一、数据层（Rust）

### 数据库
- `messages` 表新增 `media_ref TEXT`，`PRAGMA user_version` 升到 2。
- 迁移幂等：先检查列是否存在，再执行 `ALTER TABLE`；旧数据保持不变，`media_ref` 为 NULL。
- `media_ref` 只存桥返回的媒体线索（文件名、MD5、相对路径），不存绝对路径，不存媒体内容。
- `NormalizedMessage` 与 `MessageItem` 增加 `media_ref: Option<String>`，`MessageItem` 另增 `has_media: bool`。

### 同步
- `bridge.rs::normalize_message` 增加对媒体线索字段的提取。字段名以摸底结果为准，不凭猜测。
- 导入 JSONL 的消息没有媒体线索，`media_ref` 为 NULL。

### `media.rs`（新模块）
- `resolve(account, chat, local_id) -> MediaLocation`：在微信数据根目录内定位原文件，只读。解析后的路径必须位于微信数据根目录内，否则拒绝，防止路径穿越。
- `decode_image(path) -> Vec<u8>`：解码 `.dat` 为标准图片字节。支持范围由摸底结果决定。
- 缩略图缓存目录：`<app_data>/media-cache/<account>/`；提供清理函数。
- 错误分类：`Missing`（文件已被微信清理）、`Unsupported`（暂不支持的格式）、`Denied`（路径越界）。

### Tauri 命令与协议
- `get_media_thumb(account_id, chat_id, local_id)`：返回缩略图。
- `get_media_url(account_id, chat_id, local_id)`：返回可播放地址。
- 自定义协议 `chatmedia://`：流式提供视频，支持 HTTP Range，使 `<video>` 可拖动进度。
- `clear_media_cache(account_id)`：清理缩略图缓存。
- `tauri.conf.json` 的 CSP 为 `img-src` 与 `media-src` 加入该协议。

### 隐私
- 日志不记录媒体路径、文件名或哈希。
- 应用只读微信目录，只写自己的数据目录。

## 二、前端：媒体预览

- `MediaBubble`：图片显示缩略图；视频显示封面、播放按钮、时长。用 IntersectionObserver 在进入视口后才请求缩略图。
- `Lightbox`：图片可缩放；视频可播放；左右键在当前聊天已加载的媒体之间切换；Esc 关闭；焦点陷阱与 `aria` 标注。
- 缺失或解码失败：显示“媒体已不在本机”或“暂不支持此图片格式”的占位，不抛错。
- `ChatDetail` 时间线按 `kind` 渲染：文本走原逻辑；图片与视频走 `MediaBubble`。

## 三、界面焕新

- 在 `styles.css` 重建设计令牌：配色、圆角（14–20px）、阴影、间距；深色模式同步重做。
- 侧栏改为浅暖色，选中项用橙色淡底，去掉左侧竖条。
- 总览、聊天详情、数据管理、引导页统一使用新卡片样式。
- ECharts 配色改为橙色系，坐标轴与分割线更柔和。
- 侧栏小标识、加载页品牌标识与应用图标保持一致。

## 四、应用图标

- 用方案 C 的 SVG（橙色渐变圆角方块 + 气泡内小风景）替换 `src-tauri/icons/app-icon.svg`。
- 用 `tauri icon` 重新生成 `icns`、`ico` 与各尺寸 PNG。
- 检查 24px 及以下尺寸是否依然清晰。

## 五、测试与验证

- Rust 单测：路径穿越被拒绝；`.dat` 解码使用自造小样本；迁移幂等；文件缺失返回 `Missing`。
- 前端测试：`MediaBubble` 的加载、成功、缺失三种状态；`Lightbox` 的键盘操作。
- 验证命令：`npm run lint`、`npm test`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- 手动验证：用真实微信数据检查图片、视频、缺失占位与深浅色模式。

## 六、实施顺序

1. 摸底：用真实数据桥输出确认媒体字段与 `.dat` 格式。
2. 数据层与 `media.rs`。
3. 前端媒体组件。
4. 全页面视觉焕新与图标。

## 风险与回退

- 主要风险是桥没有返回任何媒体线索。此时暂停并与用户商量，备选是直接扫描微信目录再按时间匹配消息。
- `.dat` 加密格式随微信版本变化。无法解码的格式一律走 `Unsupported` 占位，不影响其他功能。
