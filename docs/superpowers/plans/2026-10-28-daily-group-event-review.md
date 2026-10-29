# 日期群聊事件回顾 Implementation Plan

**Goal:** 在 Chat Insight 的「搜索与问答」中新增可验证覆盖率的日期群聊事件回顾；修复明确日期问答约束与语义索引跨日期分段问题。

**Design:** `docs/superpowers/specs/2026-10-28-daily-group-event-review-design.md`

## Global Constraints

- 先阅读并遵守设计文档，再检查实际代码；代码与文档有差异时，以当前仓库实现为准，更新计划并记录原因。
- 不使用普通 RAG Top-K 作为事件回顾输入。
- 不使用线索词过滤事件回顾片段；符合范围的文本消息必须全部进入分段处理。
- 模型只能通过当前片段中的编号引用来源，后端负责映射与验证真实 `chat_id + local_id`。
- 不把不完整结果伪装成完整结果；不完整结果不得写入完整缓存。
- 复用现有 AI 设置、云端确认、stream、cancel、`runAiTask` 与 `MessageContext`。
- 本期不新增数据库 schema migration，不做多日/周/月回顾，不分析媒体内容。
- 不删除、重置或覆盖用户已有的工作区改动；不提交与本任务无关的文件。
- 不因某个测试失败就删测试、降低断言或绕过 lint/clippy。

## 建议 Git 策略

- 先检查 `git status --short --branch`、当前分支和远端状态。
- 目标分支：`feat/daily-group-event-review`。
- 若工作区干净且可以安全基于最新 `origin/main` 创建分支，则创建/切换到目标分支。
- 若目标分支已存在，先检查其提交与工作区状态，不要强制重置。
- 若工作区有改动，不要自动 `reset --hard`、`clean -fd` 或丢弃改动；保留现有内容，并确保不把无关文件混入提交。
- 不要在未经用户明确要求的情况下执行 `git push`、合并到 main 或发布版本。

## Implementation Tasks

### Task 0 — 仓库侦察与基线

- [x] 阅读设计文档、本文档、`README.md`、相邻 specs/plans。
- [x] 在浏览器打开 `docs/prototypes/daily-group-event-review/index.html`，查看 `README.md`，理解完整、待分析、分析中、部分失败、无匹配消息等状态；将其作为视觉验收基准。
- [x] 检查 `git status`、分支、工作区改动与当前代码版本。
- [x] 追踪 `Ask.tsx`、`api.ts`、`types.ts`、`ai.ts`、`ai/commands.rs`、`ai/store.rs`、`ai/indexer.rs`、`lib.rs`、`ai_cache` 和 `MessageContext`。
- [x] 运行基线测试/构建；记录现有失败，不把基线失败误归因于新功能。
- [x] 根据当前代码更新本文档中必要的文件清单，但不改变已确认的产品目标。

**验证记录（2026-10-29）：** 工作区在 `main` 上不干净（既有 xcli/gateway 未提交改动）。保留全部改动，新建 `feat/daily-group-event-review`。基线：`npm test` 56/56；`cargo test` 96 pass / 0 fail / 2 ignored。`TextMessage` 不含 chat 元数据，事件回顾新增 `EventTextMessage`。`ask()` 原无日期解析；`chunk_messages()` 原不按 `local_date` 切段。`confirmCloud` / `runAiTask` / `ai_cache` / `MessageContext` 均按现有实现复用。未新增 schema migration。

**完成条件：** 确认实际 API、任务取消、云端确认、缓存和测试模式后再开始改动。

### Task 1 — 日期解析与普通问答硬约束

- [x] 实现可单测的明确日期解析，支持 `YYYY-MM-DD`、`YYYY年M月D日`、`今天`、`昨天`、`前天`；不猜测无年份日期。
- [x] 在普通 AI 问答后端对明确日期施加硬范围约束，并与用户日期范围取交集。
- [x] 日期与顶部范围不相交时，不允许回退到其他日期；前端显示清晰提示。
- [x] 在 `Ask.tsx` 识别“明确日期 + 事件回顾意图”，可见地切换到事件回顾、填入日期，并在必要的云端确认后自动执行。
- [x] 单测覆盖有效/无效日期、闰年、相对日期、意图识别、日期冲突和私聊筛选不误路由。

**完成条件：** 带明确日期的普通问答不可能引用日期外的消息；事件意图能正确进入事件回顾。

### Task 2 — 精确数据读取与估算

- [x] 在 AI 数据访问层新增按 `account_id + local_date + chat_kind='group' + kind='text' + text<>''` 查询的能力。
- [x] 支持全部群聊与指定群聊；禁止 `LIMIT` 截断候选消息。
- [x] 返回稳定排序的完整消息，保留原始 `local_id`、时间、发送者、正文、群聊名称与 ID。
- [x] 新增本地估算结果：群聊数、消息数、字符数、片段数。
- [x] 新增只覆盖目标群聊范围的事件缓存指纹。
- [x] 使用 fixture DB 测试：日期前后排除、私聊排除、指定群聊隔离、空文本排除、消息数超过普通 RAG 来源上限时仍完整返回。

**完成条件：** 数据层可以证明事件回顾输入来自精确 SQL 范围，而不是语义检索。

### Task 3 — 事件抽取核心

- [x] 新增 `src-tauri/src/ai/events.rs` 与序列化类型。
- [x] 实现稳定分段器：按群聊独立分段；控制字符数/上下文长度；不截断正文、不跳过无关键词片段。
- [x] 证明每条符合条件的消息恰好归属一个片段。
- [x] 为每条消息添加片段内编号，要求模型返回 JSON 与 `source_indices`。
- [x] 校验 JSON 结构、类别、标题、source index 与来源映射。
- [x] 实现去重、合并来源、时间排序和未来安排标注。
- [x] 处理模型 JSON 失败、来源校验失败、取消和可重试错误；统计总片段、完成片段、失败片段、处理消息数。
- [x] 不完整或已取消结果不缓存；只有完全成功结果可进入 `ai_cache`。
- [x] 添加 Rust 单元测试与 fixture 测试，使用 fake LLM，不依赖真实模型/API。

**完成条件：** 事件结果的每个来源均可追溯，完整性统计准确，取消/失败不会伪装成功。

### Task 4 — Tauri 命令、API 与缓存

- [x] 在 `ai/commands.rs` 新增估算命令和事件回顾命令。
- [x] 复用现有 AI 设置校验、`ensure_allowed`、`AiState::stream`、取消和缓存工具。
- [x] 在 `ai/mod.rs`、`lib.rs` 注册模块与命令。
- [x] 在 `src/types.ts` 增加 estimate/result/event/source 类型。
- [x] 在 `src/api.ts` 增加 estimate/review API。
- [x] 缓存 key 区分账户、日期、群聊范围与模型；fingerprint 必须匹配实际 group-only 范围。
- [x] 云端确认前先做本地估算，确认内容包含日期、范围、消息数、字符数、片段数和目标 host。
- [x] 测试缓存命中、数据变更失效、强制刷新、不完整结果不缓存、取消行为。

**完成条件：** 命令完整注册，前后端类型一致，隐私确认与取消行为可验证。

### Task 5 — 事件回顾 UI

- [x] 在 `Ask.tsx` 增加第三个模式“事件回顾”。
- [x] 对照 `docs/prototypes/daily-group-event-review/` 还原视觉层级：筛选区、估算卡片、进度区、结果统计、事件时间线、类别标签、原话来源弹窗、失败/空状态。
- [x] 不把原型专用的“预览状态”下拉框、演示数据、模拟计时器或模拟分析行为带入生产代码。
- [x] 增加独立日期状态与独立群聊范围状态；默认当前日期 + 全部群聊。
- [x] 群聊选项仅展示 `chatKind === 'group'`。
- [x] 显示分析前估算，并调用既有 `confirmCloud`。
- [x] 显示执行进度、当前片段、停止按钮、完整/不完整/已取消状态。
- [x] 显示事件卡片、类别、时间、摘要、群聊名称与来源消息入口。
- [x] 来源入口复用 `MessageContext`，确保 `chat_id + local_id` 定位正确。
- [x] 区分“无匹配消息”“已扫描但没有识别到事件”“部分片段失败”。
- [x] 失败/取消提供“重新分析”操作。
- [x] 保持 AI 问答和关键词搜索的原有状态、结果和行为不回归。

**完成条件：** 桌面应用中可以从选择日期到查看原始来源完成整个流程。

### Task 6 — 索引日期边界修复

- [x] 修改 `ai/indexer.rs::chunk_messages()`，相邻消息 `local_date` 变化时强制切 chunk。
- [x] 添加跨日期测试，断言每个 chunk 内的所有消息日期相同。
- [x] 不改变索引 cursor、模型调用或其他分段策略。

**完成条件：** 索引日期边界 bug 被回归测试覆盖，现有索引测试继续通过。

### Task 7 — 端到端测试与回归

- [x] 添加合成数据：至少 8 个群聊、多个事件、重复通知、普通闲聊、空文本、同一 `local_id` 出现在不同群聊、跨午夜/跨日期、无效模型来源索引。
- [x] 验证数据覆盖、来源正确性、事件排序、去重、日期过滤、私聊排除、取消/失败状态与缓存策略。
- [x] 验证自动路由：日期 + 事件意图、日期 + 具体事实问题、私聊范围、无年份模糊日期。
- [x] 执行并修复全部检查：
  - `npm run lint`
  - `npm test`
  - `npm run build`
  - `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] 如果环境导致某项无法执行，记录实际命令、错误原因和未验证范围，不得声称通过。
- [x] 更新 README 或相关使用文档，说明事件回顾仅分析文本、日期/范围规则、云端确认与不完整结果含义。
- [x] 按原型检查桌面 1280×800 布局，并检查窄窗口的响应式行为；确认生产 UI 使用真实数据和真实来源跳转，不残留演示控件。
- [x] 最终检查 `git diff --check`、完整 diff、敏感信息、无关文件与缓存/生成文件。

**验证记录（2026-10-29）：**
- `npm run lint`：通过
- `npm test`：14 files / 68 tests 通过
- `npm run build`：通过（chunk size warning 为既有现象）
- `cargo fmt --check`：通过
- `cargo clippy --all-targets -- -D warnings`：通过。对既有 xcli `embed_model()` 误报加了 `#[allow(clippy::misnamed_getters)]`，不改变行为。
- `cargo test`：113 passed / 0 failed / 2 ignored（ignored 为需要在线模型的 extraction_eval）
- `git diff --check`：通过
- 原型 1280×800：已核对照筛选区、估算、分析中进度、部分失败。生产 UI 无“预览状态”下拉或虚构分析逻辑。未启动真实 Tauri 窗口走真实聊天数据视觉流程。

**完成条件：** 所有可执行验证通过，或每个无法通过/执行的项目均有真实原因和剩余风险说明。

## 最终交付要求

- 功能代码与测试完整。
- 设计文档和本文计划与实际实现一致，任务勾选真实反映完成情况。
- 最终回复列出：实现摘要、关键文件、测试命令与结果、未解决问题、当前分支和工作区状态。
- 不得仅以“代码已写好”作为完成标准；必须完成实际测试与修复循环。
