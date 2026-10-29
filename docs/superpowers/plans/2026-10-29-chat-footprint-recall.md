# Chat Footprint（聊天足迹 / 回忆定位）实施计划

需求依据：`docs/superpowers/specs/2026-10-29-chat-footprint-recall-design.md`\
原型：`docs/prototypes/chat-footprint/index.html`\
目标分支：`feat/daily-group-event-review`

## Task 0 --- 保护工作区与核对现状

-   [ ] 执行
    `git status --short --branch`，确认分支和未提交改动；保护现有工作，不用破坏性命令。
-   [ ] 阅读 Spec、Plan 和原型 README。
-   [ ] 检查 `src/views/Ask.tsx` 的现有事件回顾模式与状态管理。
-   [ ] 检查 `src/App.tsx` 的全局日期范围、`openDetail`、视图切换和
    accountId。
-   [ ] 检查 `src/views/ChatDetail.tsx` 的日期查询、分页、时间线滚动和
    AI 面板触发行为。
-   [ ] 检查
    `src/api.ts`、`src/types.ts`、`src-tauri/src/lib.rs`、`src-tauri/src/database.rs`、`src-tauri/src/models.rs`。
-   [ ] 确认 `chats.chat_kind`
    的真实枚举值，并根据真实代码更新实施细节。

## Task 1 --- SQLite 聚合查询

-   [ ] 在通用模型层定义日期/聊天聚合返回类型。
-   [ ] 在 `Database` 增加按 accountId、日期范围、可选 chat kind
    聚合的查询。
-   [ ] 使用 `messages.local_date`；统计所有消息类型，不过滤空文本。
-   [ ] 返回日期、chatId、名称、类型、消息数、首条时间、末条时间。
-   [ ] 日期倒序，日内聊天按最后消息时间倒序，使用稳定排序。
-   [ ] 复用现有索引；不新增 migration、不做 N+1 查询。
-   [ ] 添加 Rust
    测试：起止边界、媒体/空正文消息、群聊私聊、账号隔离、统计和排序。

## Task 2 --- Tauri 与前端 API

-   [ ] 增加非 AI Tauri command，例如 `query_chat_footprint`。
-   [ ] 在 `src/types.ts` 添加返回类型，在 `src/api.ts` 增加
    `api.chatFootprint(...)`。
-   [ ] 遵循项目现有命名、序列化和错误处理约定。
-   [ ] 确认调用链不触及 `src-tauri/src/ai/`，也不调用任何模型。

## Task 3 --- 聊天足迹 UI

-   [ ] 在 `Ask.tsx` 的「事件回顾」内部增加「聊天足迹」与「AI
    事件回顾」子模式。
-   [ ] 默认选中聊天足迹，现有 AI 事件回顾保持不变。
-   [ ] 默认最近 30 天，复用全局日期范围；增加全部/群聊/私聊筛选。
-   [ ] 按日期倒序组织可展开集合，默认展开最新一天。
-   [ ]
    每日显示日期、星期、聊天数、消息总数；每个聊天显示类型、名称、消息数、首末时间。
-   [ ] 实现 loading、empty、error、retry。
-   [ ] 对照原型还原视觉；不把演示数据或原型 JS 搬入生产。

## Task 4 --- 点击条目后日期定位

-   [ ] 点击聊天时传递 `accountId + chatId + localDate`，复用现有
    `ChatDetail`。
-   [ ] 详情有效查询范围为所选日期的
    `{from, to}`，顶部日期范围显示与实际查询一致。
-   [ ] 加载后自动滚动到消息时间线。
-   [ ] 复用已有分页；超过首屏上限仍可加载更多。
-   [ ] 处理离开详情、切换聊天、手动改日期时的 focus date
    清理/范围恢复。
-   [ ] 检查
    `AiSummaryPanel`、`ChatMemoryPanel`、`TopicsPanel`；从聊天足迹入口进入时禁止自动发起模型请求，但普通详情入口行为不变。
-   [ ] 确认媒体消息仍可通过现有组件查看。

## Task 5 --- 测试与回归

-   [ ] 日期范围、排序、类型筛选和日期统计正确。
-   [ ] 多账号隔离；媒体和空正文消息计数正确。
-   [ ] 展开/折叠互不影响；点击条目传递正确 chatId/date。
-   [ ] 详情自动滚动，分页可继续加载。
-   [ ] 空、错误、加载、重试状态正确。
-   [ ] 进入聊天足迹及详情路径不会发起 AI 调用。
-   [ ] AI 事件回顾、关键词搜索、AI 问答、普通聊天详情无回归。

## Task 6 --- 视觉验收

-   [ ] 浏览器打开 `docs/prototypes/chat-footprint/index.html`。
-   [ ] 在 1280×800 下核对日期分组、聊天行、类型筛选、空状态。
-   [ ] 检查窄窗口响应式布局与键盘可访问性。
-   [ ] 生产界面不得残留原型标识、虚构数据或模拟跳转。

## Task 7 --- 工程验收与交付

-   [ ] 执行仓库定义的 lint、test、build。
-   [ ] 执行
    `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`。
-   [ ] 执行
    `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`，如仓库有不同约定则按实际脚本并记录。
-   [ ] 执行 `cargo test --manifest-path src-tauri/Cargo.toml`。
-   [ ] 执行 `git diff --check`。
-   [ ] 审查 diff：无 migration、无新增 AI 调用、AI 事件回顾未被破坏。
-   [ ] 汇报改动、真实测试结果、风险与 Git 状态。
-   [ ] 不擅自 commit、push、合并或发布。

## 完成定义

Task 0--7 的所有可执行项完成、测试真实通过、视觉验收和 diff
审查完成后才能报告完成。无法执行的检查必须说明原因，不得伪造通过。
