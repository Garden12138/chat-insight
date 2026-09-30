export type ViewName = "overview" | "detail" | "data";
export type RangePreset = "30d" | "1y" | "all" | "custom";

export interface DateRange {
  from?: string;
  to?: string;
}

export interface PlatformCapabilities {
  os: string;
  arch: string;
  directWechatRead: boolean;
  bridgeBundled: boolean;
  setupState: "ready" | "needs_setup" | "bridge_missing" | "import_only";
  setupHint: string;
}

export interface AccountSummary {
  id: string;
  displayName: string;
  sourcePlatform: string;
  messageCount: number;
  chatCount: number;
  lastSyncAt?: string;
}

export interface SummaryMetrics {
  totalMessages: number;
  activeDays: number;
  chatCount: number;
  sentRatio: number;
}

export interface TrendPoint {
  bucket: string;
  count: number;
}

export interface HeatmapPoint {
  weekday: number;
  hour: number;
  count: number;
}

export interface RankedChat {
  chatId: string;
  displayName: string;
  chatKind: string;
  count: number;
}

export interface TypeCount {
  kind: string;
  count: number;
}

export interface DashboardData {
  metrics: SummaryMetrics;
  dailyTrend: TrendPoint[];
  monthlyTrend: TrendPoint[];
  heatmap: HeatmapPoint[];
  topChats: RankedChat[];
  messageTypes: TypeCount[];
}

export interface ChatListItem {
  id: string;
  displayName: string;
  chatKind: string;
  messageCount: number;
  lastMessageAt?: string;
}

export interface KeywordCount {
  word: string;
  count: number;
}

export interface MessageItem {
  localId: number;
  serverId?: string;
  occurredAt: string;
  senderName: string;
  isFromMe: boolean;
  kind: string;
  text: string;
}

export interface ChatDetailData {
  chat: ChatListItem;
  totalMessages: number;
  sentMessages: number;
  receivedMessages: number;
  firstMessageAt?: string;
  lastMessageAt?: string;
  dailyTrend: TrendPoint[];
  hourlyActivity: Array<{ hour: number; count: number }>;
  keywords: KeywordCount[];
  messages: MessageItem[];
  nextOffset?: number;
}

export interface SyncProgress {
  phase: "idle" | "preparing" | "sessions" | "messages" | "complete" | "error" | "cancelled";
  current: number;
  total: number;
  message: string;
  warnings: number;
  running: boolean;
}

export interface BootstrapResult {
  ready: boolean;
  message: string;
  switched: boolean;
  accountId?: string;
}

export interface SyncResult {
  accountId: string;
  chatsProcessed: number;
  messagesImported: number;
  warnings: number;
  cancelled: boolean;
}

export interface ImportResult {
  accountId: string;
  chatsImported: number;
  messagesImported: number;
}

export interface ExportResult {
  chatsExported: number;
  messagesExported: number;
}
