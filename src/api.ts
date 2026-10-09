import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AccountSummary,
  AiCacheMode,
  AiIndexOutcome,
  AiIndexProgress,
  AiIndexStatus,
  AiSettingsInput,
  AiSettingsView,
  AiStreamEvent,
  AiTestResult,
  AskResult,
  BootstrapResult,
  ChatDetailData,
  ChatListItem,
  ContentEstimate,
  DashboardData,
  DateRange,
  ExportResult,
  ImportResult,
  MediaInfo,
  MessageItem,
  PlatformCapabilities,
  ReportResult,
  SearchHit,
  SummaryResult,
  SyncProgress,
  SyncResult,
  TopicsResult,
  YearInsights,
} from "./types";
import { defaultAiSettings } from "./aiDefaults";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const browserCapabilities: PlatformCapabilities = {
  os: "browser",
  arch: "unknown",
  directWechatRead: false,
  bridgeBundled: false,
  setupState: "import_only",
  setupHint: "浏览器预览模式仅展示界面；请在桌面应用中导入数据。",
};

async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!inTauri()) throw new Error("此操作只能在 Chat Insight 桌面应用中使用");
  return invoke<T>(command, args);
}

export const api = {
  capabilities: async () => (inTauri() ? call<PlatformCapabilities>("get_capabilities") : browserCapabilities),
  accounts: async () => (inTauri() ? call<AccountSummary[]>("list_accounts") : []),
  bootstrap: () => call<BootstrapResult>("bootstrap_bridge"),
  switchAccount: () => call<BootstrapResult>("switch_wechat_account"),
  sync: () => call<SyncResult>("start_sync"),
  cancelSync: () => call<void>("cancel_sync"),
  dashboard: (accountId: string, range: DateRange) =>
    call<DashboardData>("query_dashboard", { accountId, range }),
  chats: (accountId: string, search = "") =>
    call<ChatListItem[]>("list_chats", { accountId, search }),
  chatDetail: (accountId: string, chatId: string, range: DateRange, offset = 0, limit = 60) =>
    call<ChatDetailData>("query_chat_detail", { accountId, chatId, range, offset, limit }),
  mediaInfo: (accountId: string, chatId: string, localId: number, kind: string) =>
    call<MediaInfo>("media_info", { accountId, chatId, localId, kind }),
  importJsonl: (path: string) => call<ImportResult>("import_jsonl", { path }),
  exportJsonl: (accountId: string, path: string) =>
    call<ExportResult>("export_jsonl", { accountId, path }),
  clearAccount: (accountId: string) => call<void>("clear_account", { accountId }),
  onSyncProgress: async (handler: (progress: SyncProgress) => void): Promise<UnlistenFn> => {
    if (!inTauri()) return () => undefined;
    return listen<SyncProgress>("sync-progress", (event) => handler(event.payload));
  },

  searchMessages: (accountId: string, query: string, range: DateRange, chatId?: string, limit = 100) =>
    call<SearchHit[]>("search_messages", { accountId, query, chatId: chatId || null, range, limit }),
  messageContext: (accountId: string, chatId: string, localId: number, radius = 6) =>
    call<MessageItem[]>("message_context", { accountId, chatId, localId, radius }),

  aiSettings: async () => (inTauri() ? call<AiSettingsView>("ai_get_settings") : defaultAiSettings),
  saveAiSettings: (input: AiSettingsInput) => call<AiSettingsView>("ai_save_settings", { input }),
  testAi: () => call<AiTestResult>("ai_test_connection"),
  aiIndexStatus: (accountId: string) => call<AiIndexStatus>("ai_index_status", { accountId }),
  buildAiIndex: (accountId: string, rebuild: boolean, cloudConsent: boolean) =>
    call<AiIndexOutcome>("ai_build_index", { accountId, rebuild, cloudConsent }),
  cancelAiIndex: () => call<void>("ai_cancel_index"),
  cancelAi: (requestId: string) => call<void>("ai_cancel", { requestId }),
  aiEstimate: (accountId: string, range: DateRange, chatId?: string) =>
    call<ContentEstimate>("ai_estimate", { accountId, chatId: chatId || null, range }),
  ask: (accountId: string, question: string, range: DateRange, chatId: string | undefined, requestId: string, cloudConsent: boolean) =>
    call<AskResult>("ai_ask", { accountId, question, chatId: chatId || null, range, requestId, cloudConsent }),
  summarizeChat: (accountId: string, chatId: string, range: DateRange, requestId: string, mode: AiCacheMode, cloudConsent = false) =>
    call<SummaryResult | null>("ai_summarize_chat", { accountId, chatId, range, requestId, mode, cloudConsent }),
  chatTopics: (accountId: string, chatId: string, range: DateRange, requestId: string, mode: AiCacheMode, cloudConsent = false) =>
    call<TopicsResult | null>("ai_chat_topics", { accountId, chatId, range, requestId, mode, cloudConsent }),
  yearInsights: (accountId: string, year?: string) =>
    call<YearInsights>("query_year_insights", { accountId, year: year || null }),
  yearReport: (accountId: string, year: string, requestId: string, mode: AiCacheMode, anonymize: boolean) =>
    call<ReportResult | null>("ai_year_report", { accountId, year, requestId, mode, anonymize }),
  onAiStream: async (handler: (event: AiStreamEvent) => void): Promise<UnlistenFn> => {
    if (!inTauri()) return () => undefined;
    return listen<AiStreamEvent>("ai-stream", (event) => handler(event.payload));
  },
  onAiIndexProgress: async (handler: (progress: AiIndexProgress) => void): Promise<UnlistenFn> => {
    if (!inTauri()) return () => undefined;
    return listen<AiIndexProgress>("ai-index-progress", (event) => handler(event.payload));
  },
};
