export type ViewName = "overview" | "detail" | "ask" | "report" | "data";
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

export interface MediaInfo {
  image: boolean;
  video: boolean;
  poster: boolean;
  reason?: string;
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

export interface AiSettings {
  enabled: boolean;
  baseUrl: string;
  chatModel: string;
  embedModel: string;
  allowCloud: boolean;
}

export interface AiSettingsView extends AiSettings {
  hasApiKey: boolean;
  isLocal: boolean;
  host: string;
}

export interface AiSettingsInput extends AiSettings {
  /** Omit to keep the stored key; an empty string removes it. */
  apiKey?: string;
}

export interface AiTestResult {
  models: string[];
  chatModelFound: boolean;
  embeddingDimension: number;
}

export interface AiIndexStatus {
  model: string;
  running: boolean;
  indexedChunks: number;
  pendingMessages: number;
  totalTextMessages: number;
}

export interface AiIndexProgress {
  running: boolean;
  phase: "indexing" | "complete" | "cancelled" | "error";
  current: number;
  total: number;
  chunks: number;
  message: string;
}

export interface AiIndexOutcome {
  chats: number;
  chunks: number;
  cancelled: boolean;
}

export interface AiStreamEvent {
  requestId: string;
  delta: string;
  status?: string;
}

export interface ContentEstimate {
  messages: number;
  chars: number;
}

export interface SearchHit {
  chatId: string;
  chatName: string;
  chatKind: string;
  localId: number;
  occurredAt: string;
  senderName: string;
  isFromMe: boolean;
  kind: string;
  text: string;
}

export interface AskSource {
  index: number;
  chatId: string;
  chatName: string;
  chatKind: string;
  localId: number;
  endLocalId: number;
  startAt: string;
  endAt: string;
  text: string;
}

export interface AskResult {
  answer: string;
  sources: AskSource[];
}

export type AiCacheMode = "cached" | "generate" | "refresh";

export interface SummaryResult {
  text: string;
  messageCount: number;
  cached: boolean;
  generatedAt?: string;
}

export interface Topic {
  name: string;
  description: string;
  share: number;
  chunkCount: number;
  firstAt: string;
  lastAt: string;
  sample: string;
}

export interface TopicsResult {
  topics: Topic[];
  chunkCount: number;
  cached: boolean;
  generatedAt?: string;
}

export interface RelationshipInsight {
  chatId: string;
  displayName: string;
  chatKind: string;
  count: number;
  previousCount: number;
  trend: "new" | "warming" | "cooling" | "steady";
  initiatedByMe?: number;
  myReplyMinutes?: number;
  theirReplyMinutes?: number;
}

export interface RelationshipChange {
  chatId: string;
  displayName: string;
  chatKind: string;
  count: number;
  previousCount: number;
}

export interface YearInsights {
  year: string;
  availableYears: string[];
  totalMessages: number;
  sentMessages: number;
  activeDays: number;
  chatCount: number;
  previousTotal: number;
  longestStreak?: { days: number; start: string; end: string };
  busiestDay?: TrendPoint;
  busiestMonth?: TrendPoint;
  busiestHour?: { hour: number; count: number };
  latestNight?: { occurredAt: string; chatName: string };
  monthly: TrendPoint[];
  topChats: RankedChat[];
  messageTypes: TypeCount[];
  relationships: RelationshipInsight[];
  cooling: RelationshipChange[];
}

export interface ReportResult {
  text: string;
  anonymized: boolean;
  cached: boolean;
  generatedAt?: string;
}
