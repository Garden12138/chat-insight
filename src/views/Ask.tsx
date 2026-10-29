import { Fragment, useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { api } from "../api";
import { formatChars, runAiTask, stopAiTask, useAi, useAiTask, useSessionState } from "../ai";
import { footprintQueryRange } from "../chatFootprint";
import { withEmoji } from "../emoji";
import {
  dateIntersectsRange,
  formatReviewDate,
  parseExactDate,
  routeEventReview,
} from "../eventReviewDate";
import type {
  AiIndexStatus,
  RangePreset,
  AskResult,
  AskSource,
  ChatListItem,
  DateRange,
  EventCategory,
  EventItem,
  EventReviewEstimate,
  EventReviewResult,
  SearchHit,
} from "../types";
import { ChatFootprint } from "../components/ChatFootprint";
import { DateField } from "../components/DateField";
import { Icon } from "../components/Icons";
import { Markdown } from "../components/Markdown";
import { MessageContext } from "../components/MessageContext";
import { EmptyState, InlineNotice, LoadingState } from "../components/States";
import { errorMessage, formatCount, formatDateTime, toLocalDate } from "../utils";

interface AskProps {
  accountId: string;
  range: DateRange;
  preset?: RangePreset;
  revision: number;
  onOpenDetail: (chatId: string, localDate?: string) => void;
  onPreferThirtyDays?: () => void;
  onCommitRange?: (range: DateRange) => void;
  onOpenSettings: () => void;
}

type Mode = "ask" | "search" | "review";
type ReviewPane = "footprint" | "events";

interface ContextTarget {
  chatId: string;
  chatName: string;
  localId: number;
  endLocalId?: number;
}

const EXAMPLES = ["最近有人推荐过好吃的餐厅吗？", "上次说要一起旅行是去哪里？", "我答应过别人要做什么事？"];
const DATE_RANGE_EMPTY = "问题中的日期与顶部日期范围没有交集，没有可检索的聊天。";
const CATEGORY_LABELS: Record<EventCategory, string> = {
  announcement: "通知",
  decision: "决策",
  task_progress: "进展",
  issue: "风险",
  plan: "安排",
  information: "信息",
};

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function highlight(text: string, query: string): ReactNode[] {
  const terms = query.split(/\s+/).filter(Boolean).map(escapeRegExp);
  if (!terms.length) return [text];
  const pattern = new RegExp(`(${terms.join("|")})`, "gi");
  return text.split(pattern).map((part, index) => (
    index % 2 === 1 ? <mark key={index}>{withEmoji(part)}</mark> : <Fragment key={index}>{withEmoji(part)}</Fragment>
  ));
}

function eventTime(value?: string) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value.slice(11, 16) || value;
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function parseSegmentProgress(status: string) {
  const match = status.match(/第\s*(\d+)\s*\/\s*(\d+)/);
  if (!match) return undefined;
  return { current: Number(match[1]), total: Number(match[2]) };
}

function chatKindLabel(kind: string) {
  return kind === "group" ? "群聊" : "私聊";
}

function reviewScopeLabel(chats: ChatListItem[], chatId: string) {
  if (!chatId) return "全部聊天";
  const chat = chats.find((item) => item.id === chatId);
  if (!chat) return "指定聊天";
  return `${chatKindLabel(chat.chatKind)}「${chat.displayName}」`;
}

export function Ask({ accountId, range, preset, revision, onOpenDetail, onOpenSettings, onPreferThirtyDays, onCommitRange }: AskProps) {
  const { settings, confirmCloud } = useAi();
  const [pickedMode, setMode] = useSessionState<Mode | undefined>(`ask-mode:${accountId}`, undefined);
  const [reviewPane, setReviewPane] = useSessionState<ReviewPane>(`ask-review-pane:${accountId}`, "footprint");
  const mode = pickedMode ?? (settings.enabled ? "ask" : "search");
  const [query, setQuery] = useSessionState(`ask-query:${accountId}`, "");
  const [chatId, setChatId] = useSessionState(`ask-chat:${accountId}`, "");
  const [reviewDate, setReviewDate] = useSessionState(`ask-review-date:${accountId}`, toLocalDate(new Date()));
  const [reviewChatId, setReviewChatId] = useSessionState(`ask-review-chat:${accountId}`, "");
  const [routeNotice, setRouteNotice] = useState("");
  const [dateConflict, setDateConflict] = useState("");
  const [chats, setChats] = useState<ChatListItem[]>([]);
  const [indexStatus, setIndexStatus] = useState<AiIndexStatus>();
  const [hits, setHits] = useSessionState<SearchHit[] | undefined>(`ask-hits:${accountId}`, undefined);
  const [searchedFor, setSearchedFor] = useSessionState(`ask-searched:${accountId}`, "");
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [estimate, setEstimate] = useState<EventReviewEstimate>();
  const [estimateError, setEstimateError] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<EventCategory | "all">("all");
  const [context, setContext] = useState<ContextTarget>();
  const askKey = `ask:${accountId}`;
  const reviewKey = `events:${accountId}`;
  const stream = useAiTask<AskResult>(askKey);
  const review = useAiTask<EventReviewResult>(reviewKey);
  const answer = stream.result;
  const asked = stream.label;
  const error = mode === "ask" ? stream.error : mode === "search" ? searchError : review.error;
  const groupChats = useMemo(() => chats.filter((chat) => chat.chatKind === "group"), [chats]);
  const directChats = useMemo(() => chats.filter((chat) => chat.chatKind !== "group"), [chats]);
  const selectedChat = chats.find((chat) => chat.id === chatId);

  useEffect(() => {
    api.chats(accountId).then(setChats).catch(() => setChats([]));
  }, [accountId, revision]);

  useEffect(() => {
    if (!settings.enabled || (mode === "review" && reviewPane === "footprint")) return;
    api.aiIndexStatus(accountId).then(setIndexStatus).catch(() => setIndexStatus(undefined));
  }, [accountId, settings.enabled, settings.embedModel, revision, mode, reviewPane]);

  useEffect(() => {
    if (mode === "review" && reviewPane === "footprint") onPreferThirtyDays?.();
  }, [mode, reviewPane, onPreferThirtyDays]);

  useEffect(() => {
    if (mode !== "review" || reviewPane !== "events") return;
    let active = true;
    setEstimateError("");
    api.eventReviewEstimate(accountId, reviewDate, reviewChatId || undefined)
      .then((value) => { if (active) setEstimate(value); })
      .catch((reason) => { if (active) { setEstimate(undefined); setEstimateError(errorMessage(reason)); } });
    return () => { active = false; };
  }, [accountId, reviewDate, reviewChatId, revision, mode, reviewPane]);

  const sourcesByIndex = useMemo(() => new Map((answer?.sources ?? []).map((source) => [source.index, source])), [answer]);
  const visibleEvents = useMemo(
    () => (review.result?.events ?? []).filter((event) => categoryFilter === "all" || event.category === categoryFilter),
    [review.result, categoryFilter],
  );

  function openSource(source: AskSource) {
    setContext({ chatId: source.chatId, chatName: source.chatName, localId: source.localId, endLocalId: source.endLocalId });
  }

  function openEventSource(event: EventItem) {
    const first = event.sourceRefs[0];
    if (!first) return;
    const last = event.sourceRefs[event.sourceRefs.length - 1];
    setContext({
      chatId: first.chatId,
      chatName: first.chatName,
      localId: first.localId,
      endLocalId: first.chatId === last.chatId ? last.localId : first.localId,
    });
  }

  async function runSearch(text: string) {
    setSearching(true);
    setSearchError("");
    try {
      setHits(await api.searchMessages(accountId, text, range, chatId, 200));
      setSearchedFor(text);
    } catch (reason) {
      setSearchError(errorMessage(reason));
    } finally {
      setSearching(false);
    }
  }

  async function runAsk(text: string) {
    const scope = chatId ? `“${chats.find((chat) => chat.id === chatId)?.displayName ?? "所选聊天"}”中` : "所有聊天中";
    const consent = await confirmCloud({
      title: "发送问题和相关聊天",
      detail: `你的问题，以及从${scope}找到的相关聊天。`,
    });
    if (consent === null) return;
    await runAiTask(askKey, (requestId) => api.ask(accountId, text, range, chatId, requestId, consent), { reset: true, label: text });
  }

  async function runReview(date = reviewDate, chat = reviewChatId, refresh = false) {
    let nextEstimate = estimate;
    if (!nextEstimate || nextEstimate.date !== date) {
      try {
        nextEstimate = await api.eventReviewEstimate(accountId, date, chat || undefined);
        setEstimate(nextEstimate);
      } catch (reason) {
        setEstimateError(errorMessage(reason));
        return;
      }
    }
    if (nextEstimate.messageCount === 0) {
      await runAiTask(reviewKey, async () => ({
        date,
        scope: chat || "all-chats",
        events: [],
        chatCount: 0,
        messageCount: 0,
        processedMessages: 0,
        totalSegments: 0,
        completedSegments: 0,
        failedSegments: 0,
        complete: true,
        cancelled: false,
        cached: false,
      }), { reset: true, label: date });
      return;
    }
    const scope = reviewScopeLabel(chats, chat);
    const consent = await confirmCloud({
      title: "发送当天聊天文本以整理事件",
      detail: `${formatReviewDate(date)} · ${scope} · ${formatCount(nextEstimate.messageCount)} 条文本 · ${formatChars(nextEstimate.chars)} · ${nextEstimate.segmentCount} 个片段。`,
    });
    if (consent === null) return;
    await runAiTask(
      reviewKey,
      (requestId) => api.reviewGroupEvents(accountId, date, chat || undefined, requestId, refresh ? "refresh" : "generate", consent),
      { reset: true, label: date },
    );
  }

  function submit(event?: FormEvent, value = query) {
    event?.preventDefault();
    const text = value.trim();
    if (!text || stream.running || searching || review.running) return;
    if (mode === "search") {
      void runSearch(text);
      return;
    }
    const route = routeEventReview(text, selectedChat);
    if (route) {
      setMode("review");
      setReviewPane("events");
      setReviewDate(route.date);
      setReviewChatId(route.chatId);
      setRouteNotice(`已识别日期 ${formatReviewDate(route.date)}，已切换到事件回顾`);
      void runReview(route.date, route.chatId);
      return;
    }
    const exactDate = parseExactDate(text);
    if (exactDate && !dateIntersectsRange(exactDate, range)) {
      setDateConflict(DATE_RANGE_EMPTY);
      return;
    }
    setDateConflict("");
    void runAsk(text);
  }

  const aiReady = settings.enabled;
  const needsIndex = aiReady && indexStatus && indexStatus.indexedChunks === 0;
  const reviewResult = review.result;
  const progress = parseSegmentProgress(review.status);
  const currentReviewScope = reviewScopeLabel(chats, reviewChatId);

  return (
    <div className="view-content ask-view">
      <section className="panel ask-panel">
        <div className="segmented ask-modes" role="tablist" aria-label="搜索方式">
          <button type="button" role="tab" aria-selected={mode === "ask"} className={mode === "ask" ? "active" : ""} onClick={() => setMode("ask")}><Icon name="sparkle" />AI 问答</button>
          <button type="button" role="tab" aria-selected={mode === "search"} className={mode === "search" ? "active" : ""} onClick={() => setMode("search")}><Icon name="search" />关键词搜索</button>
          <button type="button" role="tab" aria-selected={mode === "review"} className={mode === "review" ? "active" : ""} onClick={() => setMode("review")}><Icon name="calendar" />事件回顾</button>
        </div>

        {mode === "review" ? (
          <div className="review-switch">
            <div className="segmented" role="tablist" aria-label="回顾方式">
              <button type="button" role="tab" aria-selected={reviewPane === "footprint"} className={reviewPane === "footprint" ? "active" : ""} onClick={() => setReviewPane("footprint")}><Icon name="calendar" />聊天足迹</button>
              <button type="button" role="tab" aria-selected={reviewPane === "events"} className={reviewPane === "events" ? "active" : ""} onClick={() => setReviewPane("events")}><Icon name="sparkle" />AI 事件回顾</button>
            </div>
          </div>
        ) : null}
        {mode === "review" && reviewPane === "footprint" ? null : mode === "review" ? (
          <div className="event-review">
            <div className="event-filter-grid">
              <DateField className="event-field" label="回顾日期" value={reviewDate} onChange={setReviewDate} />
              <label className="event-field">
                <span>分析范围</span>
                <select value={reviewChatId} onChange={(event) => setReviewChatId(event.target.value)} aria-label="分析范围">
                  <option value="">全部聊天</option>
                  {groupChats.length > 0 && (
                    <optgroup label="群聊">
                      {groupChats.map((chat) => <option key={chat.id} value={chat.id}>{chat.displayName}</option>)}
                    </optgroup>
                  )}
                  {directChats.length > 0 && (
                    <optgroup label="私聊">
                      {directChats.map((chat) => <option key={chat.id} value={chat.id}>{chat.displayName}</option>)}
                    </optgroup>
                  )}
                </select>
              </label>
              <div className="event-filter-action">
                <span className="event-field-spacer" aria-hidden="true">操作</span>
                {review.running
                  ? <button type="button" className="secondary-button" onClick={() => stopAiTask(reviewKey)}><Icon name="stop" />停止</button>
                  : <button type="button" className="primary-button" disabled={!aiReady} onClick={() => void runReview(reviewDate, reviewChatId)}><Icon name="sparkle" />开始分析</button>}
              </div>
            </div>
            {estimateError ? <p className="subtle">{estimateError}</p> : estimate && (
              <div className="event-estimate-cards">
                <div><span>聊天</span><strong>{formatCount(estimate.chatCount)}<small>个</small></strong></div>
                <div><span>消息</span><strong>{formatCount(estimate.messageCount)}<small>条</small></strong></div>
                <div><span>片段</span><strong>{formatCount(estimate.segmentCount)}<small>段</small></strong></div>
              </div>
            )}
          </div>
        ) : (
          <>
            <form className="ask-form" onSubmit={submit}>
              <Icon name={mode === "ask" ? "sparkle" : "search"} />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={mode === "ask" ? "用一句话问问你的聊天记录，例如：上次和老王约的饭是哪天？" : "输入关键词，多个词用空格分隔"}
                aria-label={mode === "ask" ? "输入问题" : "输入关键词"}
              />
              <select value={chatId} onChange={(event) => setChatId(event.target.value)} aria-label="搜索范围">
                <option value="">全部聊天</option>
                {chats.map((chat) => <option key={chat.id} value={chat.id}>{chat.displayName}</option>)}
              </select>
              {stream.running
                ? <button type="button" className="secondary-button" onClick={() => stopAiTask(askKey)}><Icon name="stop" />停止</button>
                : <button type="submit" className="primary-button" disabled={!query.trim() || searching || (mode === "ask" && !aiReady)}>{mode === "ask" ? "提问" : "搜索"}</button>}
            </form>
            <p className="subtle ask-hint">结果受顶部日期范围限制。</p>
          </>
        )}
      </section>

      {mode === "review" && reviewPane === "footprint" && (
        <ChatFootprint
          accountId={accountId}
          range={preset === "all" ? {} : footprintQueryRange(range, false)}
          revision={revision}
          onOpenChat={onOpenDetail}
          onCommitRange={onCommitRange ?? (() => undefined)}
        />
      )}

      {routeNotice && mode === "review" && reviewPane === "events" && <InlineNotice tone="info">{routeNotice}</InlineNotice>}
      {dateConflict && mode === "ask" && <InlineNotice tone="warning">{dateConflict}</InlineNotice>}

      {((mode === "ask") || (mode === "review" && reviewPane === "events")) && !aiReady && (
        <InlineNotice tone="warning">
          <strong>{mode === "review" ? "事件回顾需要先开启 AI" : "AI 问答尚未开启"}</strong>
          <p>到「数据管理 → AI 设置」里开启。关键词搜索不用开 AI。</p>
          <button type="button" className="secondary-button notice-action" onClick={onOpenSettings}>前往 AI 设置</button>
        </InlineNotice>
      )}
      {mode === "ask" && needsIndex && (
        <InlineNotice tone="info">
          <strong>还没有建立索引</strong>
          <p>先去数据管理建立索引，提问才能按意思找到相关聊天。</p>
          <button type="button" className="secondary-button notice-action" onClick={onOpenSettings}>去建立索引</button>
        </InlineNotice>
      )}
      {error && <InlineNotice tone="error">{error}</InlineNotice>}

      {mode === "review" && reviewPane === "events" && review.running && (
        <section className="panel event-progress" aria-live="polite">
          <div className="event-progress-head">
            <span className="spinner" />
            <strong>{progress ? `正在分析 ${progress.current} / ${progress.total}` : "正在分析"}</strong>
            <button type="button" className="secondary-button" onClick={() => stopAiTask(reviewKey)}>停止</button>
          </div>
          <div className="event-progress-track"><span style={{ width: `${progress ? Math.round((progress.current / progress.total) * 100) : 8}%` }} /></div>
        </section>
      )}

      {mode === "review" && reviewPane === "events" && reviewResult && !review.running && (
        reviewResult.messageCount === 0 ? (
          <EmptyState title="这一天没有可分析的聊天文本" description="换一天，或换一个聊天范围再试试。" />
        ) : (
          <section className="panel event-results">
            <div className="event-results-heading">
              <div>
                <div className="event-title-row">
                  <h2>事件时间线</h2>
                  <span className={`event-status ${reviewResult.complete ? "success" : "warning"}`}>
                    {reviewResult.cancelled ? "已取消" : reviewResult.complete ? "已完成" : "未完成"}
                  </span>
                </div>
                <p className="subtle">{formatReviewDate(reviewResult.date)} · {currentReviewScope}</p>
              </div>
              <button type="button" className="secondary-button" onClick={() => void runReview(reviewDate, reviewChatId, true)}>重新分析</button>
            </div>
            <div className="event-summary">
              <div><span>聊天</span><strong>{formatCount(reviewResult.chatCount)}</strong></div>
              <div><span>消息</span><strong>{formatCount(reviewResult.messageCount)}</strong></div>
              <div><span>片段</span><strong>{reviewResult.completedSegments} / {reviewResult.totalSegments}</strong></div>
              <div><span>事件</span><strong>{formatCount(reviewResult.events.length)}</strong></div>
            </div>
            {(!reviewResult.complete || reviewResult.failedSegments > 0 || reviewResult.cancelled) && (
              <div className="event-partial">
                <strong>这次没有全部分析完</strong>
                <p>
                  {reviewResult.cancelled ? "分析已停止。" : ""}
                  {reviewResult.failedSegments > 0 ? `有 ${reviewResult.failedSegments} 段没有成功。` : ""}
                </p>
                <button type="button" className="text-button" onClick={() => void runReview(reviewDate, reviewChatId, true)}>重新分析</button>
              </div>
            )}
            {reviewResult.events.length === 0 ? (
              <EmptyState title="没有识别到事件" description="当天可能只是闲聊。" />
            ) : (
              <>
                <div className="event-toolbar">
                  <strong>识别到的事件 <span>{visibleEvents.length}</span></strong>
                  <div className="event-filters">
                    <button type="button" className={categoryFilter === "all" ? "active" : ""} onClick={() => setCategoryFilter("all")}>全部</button>
                    {(Object.keys(CATEGORY_LABELS) as EventCategory[]).map((category) => (
                      <button key={category} type="button" className={categoryFilter === category ? "active" : ""} onClick={() => setCategoryFilter(category)}>{CATEGORY_LABELS[category]}</button>
                    ))}
                  </div>
                </div>
                <div className="event-list">
                  {visibleEvents.map((event) => (
                    <article key={event.id} className="event-card">
                      <div className="event-time">
                        <strong>{eventTime(event.startAt)}</strong>
                        <span>{event.scheduledDate ? `安排 ${formatReviewDate(event.scheduledDate)}` : event.sourceRefs[0]?.chatName}</span>
                      </div>
                      <div className="event-main">
                        <div className="event-meta">
                          <span className={`event-category ${event.category}`}>{CATEGORY_LABELS[event.category]}</span>
                          <span className="event-chat">{event.sourceRefs[0]?.chatName}</span>
                        </div>
                        <h3>{event.title}</h3>
                        <p>{event.summary}</p>
                        <button type="button" className="event-source-link" onClick={() => openEventSource(event)}>查看原话 · {event.sourceRefs.length} 条来源</button>
                      </div>
                    </article>
                  ))}
                </div>
              </>
            )}
            <p className="event-foot">总结可能有误，可点「查看原话」核对。</p>
          </section>
        )
      )}

      {mode === "ask" ? (
        asked ? (
          <section className="panel answer-panel" aria-live="polite">
            <div className="panel-heading"><div><p className="eyebrow">ANSWER</p><h2>{asked}</h2></div></div>
            <div className="answer-body">
              {(answer?.answer || stream.text)
                ? <Markdown text={answer?.answer || stream.text} onCitation={(index) => { const source = sourcesByIndex.get(index); if (source) openSource(source); }} />
                : stream.running && <LoadingState label={stream.status || "正在思考…"} />}
              {stream.running && stream.text && <p className="stream-status"><span className="spinner" />{stream.status || "正在生成…"}</p>}
            </div>
            {answer && answer.sources.length > 0 && (
              <div className="source-list">
                <p className="eyebrow">SOURCES</p>
                {answer.sources.map((source) => (
                  <button key={source.index} type="button" className="source-card-item" onClick={() => openSource(source)}>
                    <span className="source-index">{source.index}</span>
                    <span className="source-body">
                      <strong>{source.chatName}<small>{formatDateTime(source.startAt)}</small></strong>
                      <span>{withEmoji(source.text.split("\n").slice(0, 3).join(" / "))}</span>
                    </span>
                    <Icon name="chevron" />
                  </button>
                ))}
              </div>
            )}
          </section>
        ) : aiReady && (
          <section className="panel ask-examples">
            <div className="panel-heading"><div><p className="eyebrow">TRY ASKING</p><h2>可以这样问</h2></div></div>
            <div className="example-list">
              {EXAMPLES.map((example) => <button key={example} type="button" onClick={() => { setQuery(example); submit(undefined, example); }}>{example}</button>)}
            </div>
          </section>
        )
      ) : mode === "search" ? (
        searching ? <LoadingState label="正在搜索…" /> : hits && (
          hits.length ? (
            <section className="panel hit-panel">
              <div className="panel-heading"><div><p className="eyebrow">RESULTS</p><h2>找到 {hits.length >= 200 ? "200+" : hits.length} 条消息</h2></div><span className="subtle">按时间从新到旧</span></div>
              <ol className="hit-list">
                {hits.map((hit) => (
                  <li key={`${hit.chatId}-${hit.localId}`}>
                    <button type="button" onClick={() => setContext({ chatId: hit.chatId, chatName: hit.chatName, localId: hit.localId })}>
                      <span className={`avatar ${hit.chatKind}`}>{hit.chatName.slice(0, 1).toUpperCase()}</span>
                      <span className="hit-body">
                        <span className="hit-meta"><strong>{hit.chatName}</strong><small>{hit.isFromMe ? "我" : hit.senderName || "对方"} · {formatDateTime(hit.occurredAt)}</small></span>
                        <span className="hit-text">{highlight(hit.text, searchedFor)}</span>
                      </span>
                    </button>
                  </li>
                ))}
              </ol>
            </section>
          ) : <EmptyState title="没有找到匹配的消息" description="换一个关键词，或调整顶部的日期范围再试试。" />
        )
      ) : null}

      {context && (
        <MessageContext
          accountId={accountId}
          chatId={context.chatId}
          chatName={context.chatName}
          localId={context.localId}
          endLocalId={context.endLocalId}
          onOpenDetail={(id) => { setContext(undefined); onOpenDetail(id); }}
          onClose={() => setContext(undefined)}
        />
      )}
    </div>
  );
}
