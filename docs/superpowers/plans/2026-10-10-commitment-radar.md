# 承诺雷达（约定与待办）Implementation Plan

**Goal:** 从聊天中识别“我答应的”和“对方答应我的”事，经用户确认后作为待办跟进，并能回到原话。

**Architecture:** Rust 新增 `commitments.rs`（存储与不依赖 AI 的命令）和 `ai/commitments.rs`（切窗口、预筛、
抽取、校验、扫描）。数据库升级到 v3。前端新增 `Commitments` 视图，复用 `runAiTask`、`MessageContext`
与云端确认弹窗。

**Tech Stack:** Tauri 2（Rust、rusqlite、chrono）、React 19、TypeScript、Vitest。

## Global Constraints

- AI 只产生 `suggested`；用户状态与编辑字段永不被 AI 覆盖。
- 云端每次扫描前确认；自动扫描只在本机模型下运行。
- 日志不记录聊天正文或提示词。
- 验证：`npm run lint`、`npm test`、`npm run build`、`cargo fmt --check`、`cargo clippy --all-targets -D warnings`、`cargo test`。

## File Structure

| 文件 | 职责 |
| --- | --- |
| `src-tauri/src/database.rs` | `SCHEMA_V3` 迁移、旧数据集表清单 |
| `src-tauri/src/commitments.rs`（新） | 类型、增改查、计数、已扫描区间 |
| `src-tauri/src/ai/commitments.rs`（新） | 窗口、预筛、提示词、解析、校验、扫描 |
| `src-tauri/src/ai/commands.rs`、`ai/mod.rs`、`ai/settings.rs`、`lib.rs` | 命令、扫描锁、自动扫描设置 |
| `src-tauri/testdata/commitments/`（新） | 合成对话与期望结果 |
| `src/types.ts`、`src/api.ts` | 类型与调用 |
| `src/views/Commitments.tsx`（新） | 约定与待办页面 |
| `src/App.tsx`、`src/components/AiSettings.tsx`、`src/guide.ts`、`src/styles.css` | 导航、角标、自动扫描、设置、指引 |

### Task 1: 数据库 v3
- [ ] 写迁移测试（v2 → v3、两次初始化幂等、旧数据集迁移包含新表），实现，通过。

### Task 2: 存储层
- [ ] 区间合并纯函数与测试；插入、去重、列表、更新、计数、AI 更新只写 `ai_*`。

### Task 3: 抽取层
- [ ] 窗口、预筛、提示词、解析、校验的纯函数测试；`FakeClient` 驱动的扫描流程测试（增量、取消续扫、跟进更新）。

### Task 4: 命令与设置
- [ ] 注册命令；扫描锁；`auto_scan_commitments` 默认关闭、非本机时保存为关闭。

### Task 5: 前端
- [ ] 视图、导航角标、同步后自动扫描、AI 设置开关、使用指引；组件测试。

### Task 6: 评估、文档与发布
- [ ] 合成 fixture 与 `#[ignore]` 评估；README；版本 0.2.0；全量验证。
