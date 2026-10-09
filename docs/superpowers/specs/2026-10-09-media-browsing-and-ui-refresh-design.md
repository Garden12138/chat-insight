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

### 摸底结论（2026-10-09，修订本节）
- 内置数据桥 `wechat-cli v1.6.20` 已提供 `media <chat> --local-id N`，返回 JSON：
  `data.media[0].images[].path`（已解码的 `.jpg`，位于 `~/.wechat-cli/media-cache/<wxid>/`）、
  `data.media[0].resources[]`（`type` 为 `image`/`video`，`path` 为绝对路径，视频为 `.mp4`，
  未下载的视频只有 `<md5>_thumb.jpg` 封面）。单次调用约 80 ms。
- 现有同步调用的 `timeline` 默认已会把图片解码到同一缓存目录，因此按需调用 `media` 不改变隐私边界。
- 结论：不改数据库、不自写 `.dat` 解码、不另建缩略图缓存。用 `chat_id + local_id` 按需向数据桥查询即可。

### `media.rs`（新模块）
- `parse_media(value) -> MediaPaths { image, video, poster }`：从 `media` 输出中按类型和扩展名分类路径。
- `MediaResolver`：调用数据桥并以 `(account_id, chat_id, local_id)` 为键缓存解析结果（进程内）。
- 账号校验：读取数据桥配置算出当前账号 ID，与请求的数据集 ID 不一致时拒绝（导入的数据集或其他账号没有媒体）。
- 路径白名单：文件必须存在、扩展名属于图片/视频白名单，且规范化后位于 `xwechat_files` 目录或 `~/.wechat-cli/media-cache` 内。
- 错误以中文原因返回：不是当前连接的账号、媒体已不在本机、不支持的类型。

### Tauri 命令与协议
- 命令 `media_info(account_id, chat_id, local_id) -> MediaInfo { image, video, poster, reason }`。
- 自定义协议 `chatmedia`：`chatmedia://localhost/<variant>/<account>/<chat>/<local_id>`（Windows 为
  `http://chatmedia.localhost/...`），`variant` 为 `image`/`video`/`poster`；支持 HTTP Range，使 `<video>` 可拖动。
- `tauri.conf.json` 的 CSP 为 `img-src` 与新增的 `media-src` 加入该协议。

### 隐私
- 日志不记录媒体路径、文件名或哈希。
- 应用只读微信目录与数据桥缓存，只写自己的数据目录；网页只能按消息身份请求媒体，无法读取任意路径。

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

- Rust 单测：`media` 输出解析（图片、已下载视频、仅封面视频）；白名单外路径被拒绝；Range 解析。
- 前端测试：`MediaBubble` 的加载、成功、缺失三种状态；`Lightbox` 的键盘操作。
- 验证命令：`npm run lint`、`npm test`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- 手动验证：用真实微信数据检查图片、视频、缺失占位与深浅色模式。

## 六、实施顺序

1. 摸底（已完成，结论见第一节）。
2. `media.rs`、命令与协议。
3. 前端媒体组件。
4. 全页面视觉焕新与图标。

## 风险与回退

- 图片密钥失效时数据桥会自动运行 `wxkey image-key` 刷新，仍失败则返回无路径，界面显示占位。
- WebKit 对自定义协议的视频播放依赖正确的 Range 响应；若个别格式无法播放，占位提示“在微信中打开”。
