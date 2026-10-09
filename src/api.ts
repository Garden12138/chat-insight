import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AccountSummary,
  BootstrapResult,
  ChatDetailData,
  ChatListItem,
  DashboardData,
  DateRange,
  ExportResult,
  ImportResult,
  MediaInfo,
  PlatformCapabilities,
  SyncProgress,
  SyncResult,
} from "./types";

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
  mediaInfo: (accountId: string, chatId: string, localId: number) =>
    call<MediaInfo>("media_info", { accountId, chatId, localId }),
  importJsonl: (path: string) => call<ImportResult>("import_jsonl", { path }),
  exportJsonl: (accountId: string, path: string) =>
    call<ExportResult>("export_jsonl", { accountId, path }),
  clearAccount: (accountId: string) => call<void>("clear_account", { accountId }),
  onSyncProgress: async (handler: (progress: SyncProgress) => void): Promise<UnlistenFn> => {
    if (!inTauri()) return () => undefined;
    return listen<SyncProgress>("sync-progress", (event) => handler(event.payload));
  },
};
