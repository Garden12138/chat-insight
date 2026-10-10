# 群聊决策记录与聊天里的约定面板 Implementation Plan

**Goal:** 群聊决策记录（持久化、可确认、可推翻或解决、可回到原话），以及聊天详情里的“约定与决策”面板。

**Architecture:** 把承诺雷达的通用抽取部分拆到 `ai/extraction.rs`，约定与决策各实现一个 `Extractor`；
已扫描区间改为按 `kind` 存在 `memory_scans`。前端抽出 `MemoryCard`，新增 `ChatMemoryPanel`。

## Global Constraints

- AI 只产生 `suggested`；用户状态与编辑字段永不被 AI 覆盖。
- 决策扫描只手动触发；云端每次确认。
- 约定已有测试不修改即可通过。
- 验证：`npm run lint`、`npm test`、`npm run build`、`cargo fmt --check`、`cargo clippy --all-targets -D warnings`、`cargo test`。

## File Structure

| 文件 | 职责 |
| --- | --- |
| `src-tauri/src/memory.rs`（新） | 按 kind 的已扫描区间、标题与日期清洗、上下文读取 |
| `src-tauri/src/ai/extraction.rs`（新） | 窗口、计划、扫描循环、`Extractor`、JSON 解析 |
| `src-tauri/src/ai/commitments.rs` | 约定抽取器 |
| `src-tauri/src/decisions.rs`（新）、`src-tauri/src/ai/decisions.rs`（新） | 决策存储与抽取器 |
| `src-tauri/src/database.rs` | v4 迁移 |
| `src-tauri/src/ai/extraction_eval.rs`（由 `commitments_eval.rs` 改名） | 约定与决策评估 |
| `src/components/MemoryCard.tsx`（新） | 通用卡片 |
| `src/components/ChatMemoryPanel.tsx`（新） | 聊天详情里的约定与决策 |

### Task 1: 拆分抽取核心
- [ ] 移动通用代码，约定测试原样通过。

### Task 2: 数据库 v4
- [ ] `memory_scans` 迁移与 `decisions` 表；迁移测试。

### Task 3: 决策后端
- [ ] 存储、抽取器、命令；校验与流程测试。

### Task 4: 前端
- [ ] `MemoryCard`、`ChatMemoryPanel`、接入与测试。

### Task 5: 评估、文档与发布
- [ ] 决策样本与评估；README；0.3.0；全量验证。
