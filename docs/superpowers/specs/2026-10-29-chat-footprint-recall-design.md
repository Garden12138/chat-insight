# Chat Footprint（聊天足迹 / 回忆定位）产品与技术规范

-   目标仓库：`Garden12138/chat-insight`
-   目标分支：`feat/daily-group-event-review`
-   版本：v1.0
-   日期：2026-10-29

## 1. 背景与目标

当前「搜索与问答」页面已有 AI
事件回顾。用户有时只想回忆某天与哪些群聊、好友有过交流，并快速返回当天聊天详情；此场景不需要
AI 总结、关键词识别或事件提取。

新增 **聊天足迹（Chat
Footprint）**，作为现有「事件回顾」中的一个子模式。它是本地数据库驱动的日期浏览与定位功能，不是
AI 事件识别功能。

用户可以选择日期范围，查看每天的聊天集合；展开日期后看到当天有消息的所有群聊和私聊；点击某个聊天后打开现有聊天详情，将范围限定到所选日期，并自动滚动到消息时间线。

## 2. 非目标

-   不调用 AI / LLM / embedding / xcli 服务。
-   不生成事件标题、事件摘要或智能分类。
-   不做关键词匹配、规则事件识别或语义检索。
-   不新增数据库表或 schema migration。
-   不复制实现一套新的聊天详情页。
-   不更改或移除现有 AI 事件回顾功能。
-   不读取未同步的聊天内容，也不把媒体转换为文本。

## 3. 入口与界面

在 `src/views/Ask.tsx` 当前「事件回顾」模式内部增加两个子模式：

-   **聊天足迹**：默认选中，完全不依赖 AI。
-   **AI 事件回顾**：保留现有实现与行为。

不要增加左侧导航项，不要重做应用外壳。复用现有主题、图标、日期范围、数据集选择和聊天详情页。

聊天足迹包含：

-   标题与说明：「按日期找回你聊过的群聊与私聊」。
-   默认最近 30 天，复用现有 `DateRange` / `DateRangePicker`
    规则；支持起止日期。
-   类型筛选：全部 / 群聊 / 私聊。
-   日期集合按新到旧排序；每一天显示日期、星期、当天聊天数、当天消息总数。
-   日期可独立展开/折叠，默认展开最新一天。
-   每个聊天条目显示类型、聊天名称、当天消息数、首条时间和末条时间。
-   提供 loading、empty、error、retry 状态；查询失败不能伪装成空结果。
-   清晰说明数据来自本地已同步记录，不调用 AI。

原型位于
`docs/prototypes/chat-footprint/`。正式实现应参考原型，但使用仓库现有
React/Tauri 组件和样式，不把原型 JS 直接嵌入生产代码。

## 4. 数据与统计规则

以 `messages.local_date` 作为本地日期字段，不能用 UTC
日期重新推算，以免跨午夜的消息被分到错误日期。

每个日期 + 聊天的聚合行至少包括：

-   `localDate`
-   `chatId`
-   `displayName`
-   `chatKind`
-   `messageCount`
-   `firstMessageAt`
-   `lastMessageAt`

规则：

-   统计所有已同步消息类型，不限于
    `kind='text'`。图片、视频、语音、文件等只要在 `messages`
    表中有记录，就计为一条消息。
-   不按 `text<>''` 过滤，媒体消息可能没有文本正文。
-   日期级消息总数为当天各聊天 `messageCount`
    之和；日期级聊天数为当天不同 `chatId` 数量。
-   首末时间分别使用 `MIN(occurred_at)` / `MAX(occurred_at)`。
-   群聊/私聊筛选必须使用仓库中 `chats.chat_kind`
    的真实枚举值，不得猜测。
-   严格限制当前 `account_id`，避免账号间串数据。
-   日期使用 ISO `YYYY-MM-DD`，起止边界均包含。
-   日期按 `localDate DESC`；日内聊天按
    `lastMessageAt DESC`，同时间时使用稳定的次级排序。
-   没有消息的聊天不能出现在当天列表中。

## 5. 后端接口建议

先核对实际代码，再按项目惯例实现。建议增加通用、非 AI 的查询，不要放入
`src-tauri/src/ai/`：

-   通用 Rust 聚合结果类型，例如 `ChatFootprintEntry`。
-   `Database::chat_footprint(account_id, range, chat_kind)`。
-   Tauri command，例如 `query_chat_footprint`。
-   `src/types.ts` 对应类型。
-   `src/api.ts` 对应 `api.chatFootprint(accountId, range, chatKind?)`。

SQL 逻辑示意（须按现有 Rust 类型、参数和错误处理约定调整）：

``` sql
SELECT m.local_date, m.chat_id, c.display_name, c.chat_kind,
       COUNT(*) AS message_count,
       MIN(m.occurred_at) AS first_message_at,
       MAX(m.occurred_at) AS last_message_at
FROM messages m
JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
WHERE m.account_id=?1
  AND (?2 IS NULL OR m.local_date>=?2)
  AND (?3 IS NULL OR m.local_date<=?3)
  AND (?4 IS NULL OR c.chat_kind=?4)
GROUP BY m.local_date, m.chat_id, c.display_name, c.chat_kind
ORDER BY m.local_date DESC, last_message_at DESC, c.display_name COLLATE NOCASE ASC;
```

性能要求：默认最近 30 天；复用现有
`idx_messages_account_date(account_id, local_date)` 等索引；不做 N+1
查询；只返回聚合数据，不返回正文；评估「全部历史」大范围结果，不要一次加载整个消息表。不要新增
schema migration。

## 6. 跳转到聊天详情与日期定位

点击聊天条目时：

1.  携带 `accountId + chatId + localDate`。
2.  打开现有 `ChatDetail`，不创建新详情页。
3.  详情查询范围限定为
    `{ from: localDate, to: localDate }`，顶部日期范围控件必须与实际查询范围一致，避免控件显示「全部」但详情只查一天。
4.  数据加载后自动滚动到消息时间线面板，而不是停在顶部图表区域。
5.  复用现有消息分页；当天消息超过首屏上限时，仍可继续加载，不能静默省略。
6.  保留现有媒体预览、发送者、时间等功能。
7.  处理离开详情、切换聊天、用户修改日期范围时的 focus date
    清理/日期范围恢复，避免意外永久覆盖原全局筛选。
8.  检查 `AiSummaryPanel`、`ChatMemoryPanel`、`TopicsPanel`
    的触发行为。如果它们会在进入页面时自动发起模型请求，从聊天足迹入口进入时必须抑制这些自动生成调用；普通方式进入聊天详情的行为保持不变。此入口的定位链路不得调用
    AI。

## 7. 隐私与兼容性

-   查询严格限制当前账号，只访问本地数据库，不上传聊天内容。
-   不修改原始消息，不新增权限或微信访问方式。
-   空结果与查询失败必须区分。
-   现有 AI 事件回顾、关键词搜索、普通 AI 问答和聊天详情均不得回归。

## 8. 验收标准

### 功能

-   [ ] 默认展示最近 30 天有消息的日期，日期倒序。
-   [ ] 每天可展开/折叠，显示当天所有有消息的群聊和私聊。
-   [ ] 日期的聊天数、消息总数与明细一致。
-   [ ] 所有消息类型均计数，包括正文为空的媒体消息。
-   [ ] 全部 / 群聊 / 私聊筛选正确，日期统计同步更新。
-   [ ] 起止日期边界包含；不同账号数据隔离。
-   [ ] 点击聊天后打开正确聊天和日期的详情，并自动滚动到消息时间线。
-   [ ] 当天超过分页上限时可继续加载。
-   [ ] 从聊天足迹进入的链路不调用 AI；若详情组件会自动发起 AI
    请求，必须抑制该入口的自动请求。
-   [ ] loading、empty、error、retry 状态正确。
-   [ ] 现有 AI 事件回顾和其他页面无回归。

### 工程

-   [ ] 无 schema migration；新查询位于通用数据库层。
-   [ ] 复用现有组件、样式、详情页和分页。
-   [ ] 通过仓库 lint/test/build、Rust fmt/clippy/test 和
    `git diff --check`。
-   [ ] 最终报告提供真实测试结果、改动文件、风险与 Git 状态。
