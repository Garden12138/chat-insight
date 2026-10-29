export type ViewName = "overview" | "commitments" | "detail" | "ask" | "report" | "data";
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

export type AiProvider = "openai_compatible" | "bailian" | "xcli";

export interface AiSettings {
  provider?: AiProvider;
  enabled: boolean;
  baseUrl: string;
  chatModel: string;
  embedModel: string;
  allowCloud: boolean;
  /** Scan new messages for commitments after each sync; local models only. */
  autoScanCommitments: boolean;
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
  gateway?: GatewayPrivacy | null;
}

export interface GatewayPrivacy {
  chatCloud: boolean;
  embeddingCloud: boolean;
  chatLocation: string;
  embeddingLocation: string;
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

export type EventCategory =
  | "announcement"
  | "decision"
  | "task_progress"
  | "issue"
  | "plan"
  | "information";

export interface EventReviewEstimate {
  date: string;
  chatCount: number;
  messageCount: number;
  chars: number;
  segmentCount: number;
}

export interface EventSourceRef {
  chatId: string;
  chatName: string;
  chatKind: string;
  localId: number;
  occurredAt: string;
  senderName: string;
  isFromMe: boolean;
  text: string;
}

export interface EventItem {
  id: string;
  title: string;
  category: EventCategory;
  summary: string;
  startAt: string;
  endAt: string;
  sourceRefs: EventSourceRef[];
  scheduledDate?: string;
}

export interface EventReviewResult {
  date: string;
  scope: string;
  events: EventItem[];
  chatCount: number;
  messageCount: number;
  processedMessages: number;
  totalSegments: number;
  completedSegments: number;
  failedSegments: number;
  complete: boolean;
  cancelled: boolean;
  cached: boolean;
  generatedAt?: string;
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

export type CommitmentStatus = "suggested" | "open" | "done" | "dismissed";
export type CommitmentDirection = "mine" | "theirs";

export interface Commitment {
  id: number;
  chatId: string;
  chatName: string;
  chatKind: string;
  direction: CommitmentDirection;
  ownerName: string;
  title: string;
  quote: string;
  dueText?: string;
  dueDate?: string;
  status: CommitmentStatus;
  /** What the model concluded from later messages; the user decides. */
  aiStatus?: "open" | "done" | "cancelled" | "changed" | "unclear";
  aiNote?: string;
  followupLocalId?: number;
  sourceLocalId: number;
  sourceEndLocalId: number;
  sourceAt: string;
  confidence: number;
  userEdited: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface CommitmentFilter {
  statuses?: CommitmentStatus[];
  direction?: CommitmentDirection;
  chatId?: string;
}

export interface CommitmentPatch {
  status?: CommitmentStatus;
  edit?: { title: string; dueDate?: string };
}

export interface CommitmentCounts {
  suggested: number;
  open: number;
  overdue: number;
  done: number;
  dismissed: number;
  suggestedTotal: number;
  acceptedTotal: number;
}

export interface CommitmentEstimate {
  chats: number;
  windows: number;
  skipped: number;
  chars: number;
}

export type DecisionKind = "decision" | "question";
export type DecisionStatus = "suggested" | "active" | "closed" | "dismissed";

export interface Decision {
  id: number;
  chatId: string;
  chatName: string;
  kind: DecisionKind;
  title: string;
  ownerName?: string;
  dueDate?: string;
  quote: string;
  speakerName: string;
  status: DecisionStatus;
  aiStatus?: "overturned" | "changed" | "resolved";
  aiNote?: string;
  followupLocalId?: number;
  sourceLocalId: number;
  sourceAt: string;
  confidence: number;
  userEdited: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface DecisionFilter {
  statuses?: DecisionStatus[];
  kinds?: DecisionKind[];
  chatId?: string;
}

export interface DecisionPatch {
  status?: DecisionStatus;
  edit?: { title: string; owner?: string; dueDate?: string };
}

export interface CommitmentScanOutcome {
  chats: number;
  windows: number;
  skipped: number;
  failed: number;
  created: number;
  updated: number;
  cancelled: boolean;
}
