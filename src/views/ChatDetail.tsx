import { useEffect, useMemo, useState } from "react";
import type { EChartsOption } from "echarts";
import { api } from "../api";
import type { ChatDetailData, ChatListItem, DateRange } from "../types";
import { Chart } from "../components/Chart";
import { EmptyState, LoadingState } from "../components/States";
import { Icon } from "../components/Icons";
import { Lightbox } from "../components/Lightbox";
import { MediaBubble } from "../components/MediaBubble";
import { AiSummaryPanel } from "../components/AiSummaryPanel";
import { TopicsPanel } from "../components/TopicsPanel";
import { isMediaKind, isPreviewKind } from "../media";
import { withEmoji } from "../emoji";
import { chartTheme } from "../theme";
import { formatCount, formatDateTime, messageKindLabel } from "../utils";

interface ChatDetailProps {
  accountId: string;
  range: DateRange;
  revision: number;
  requestedChatId?: string;
}

const { axis: axisColor, grid: lineColor } = chartTheme;

export function ChatDetail({ accountId, range, revision, requestedChatId }: ChatDetailProps) {
  const [search, setSearch] = useState("");
  const [chats, setChats] = useState<ChatListItem[]>([]);
  const [selectedId, setSelectedId] = useState(requestedChatId ?? "");
  const [detail, setDetail] = useState<ChatDetailData>();
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const [viewerIndex, setViewerIndex] = useState<number>();
  const mediaMessages = useMemo(() => detail?.messages.filter((message) => isMediaKind(message.kind)) ?? [], [detail]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      api.chats(accountId, search).then((items) => {
        setChats(items);
        setSelectedId((current) => current || requestedChatId || items[0]?.id || "");
      }).catch((reason) => setError(String(reason)));
    }, 180);
    return () => window.clearTimeout(timer);
  }, [accountId, search, requestedChatId, revision]);

  useEffect(() => {
    if (!selectedId) { setLoading(false); return; }
    let active = true;
    setLoading(true);
    setError("");
    setViewerIndex(undefined);
    api.chatDetail(accountId, selectedId, range, 0, 60)
      .then((value) => active && setDetail(value))
      .catch((reason) => active && setError(String(reason)))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [accountId, selectedId, range, revision]);

  async function loadMore() {
    if (!detail?.nextOffset) return;
    setLoadingMore(true);
    try {
      const next = await api.chatDetail(accountId, selectedId, range, detail.nextOffset, 60);
      setDetail({ ...next, messages: [...detail.messages, ...next.messages] });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoadingMore(false);
    }
  }

  const trendOption = useMemo<EChartsOption>(() => ({
    tooltip: { ...chartTheme.tooltip, trigger: "axis" },
    grid: { top: 12, left: 44, right: 16, bottom: 28 },
    xAxis: { type: "category", data: detail?.dailyTrend.map((item) => item.bucket) ?? [], axisLabel: { color: axisColor, hideOverlap: true }, axisLine: { lineStyle: { color: lineColor } } },
    yAxis: { type: "value", minInterval: 1, axisLabel: { color: axisColor }, splitLine: { lineStyle: { color: lineColor } } },
    series: [{ type: "bar", data: detail?.dailyTrend.map((item) => item.count) ?? [], barMaxWidth: 18, itemStyle: { color: chartTheme.barGradient, borderRadius: [6, 6, 0, 0] } }],
  }), [detail]);

  const hourOption = useMemo<EChartsOption>(() => ({
    tooltip: { ...chartTheme.tooltip, trigger: "axis" },
    grid: { top: 12, left: 42, right: 12, bottom: 28 },
    xAxis: { type: "category", data: Array.from({ length: 24 }, (_, hour) => `${hour}`), axisLabel: { color: axisColor, interval: 2 }, axisLine: { lineStyle: { color: lineColor } } },
    yAxis: { type: "value", minInterval: 1, axisLabel: { color: axisColor }, splitLine: { lineStyle: { color: lineColor } } },
    series: [{ type: "line", data: Array.from({ length: 24 }, (_, hour) => detail?.hourlyActivity.find((item) => item.hour === hour)?.count ?? 0), showSymbol: false, smooth: true, lineStyle: { color: chartTheme.accent, width: 3 }, areaStyle: { color: chartTheme.areaGradient } }],
  }), [detail]);

  return (
    <div className="view-content detail-layout">
      <aside className="chat-browser panel">
        <div className="search-box"><Icon name="search" /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索好友或群聊" aria-label="搜索聊天" /></div>
        <div className="chat-list">
          {chats.map((chat) => (
            <button key={chat.id} className={selectedId === chat.id ? "selected" : ""} onClick={() => setSelectedId(chat.id)}>
              <span className={`avatar ${chat.chatKind}`}>{chat.displayName.slice(0, 1).toUpperCase()}</span>
              <span><strong>{chat.displayName}</strong><small>{formatCount(chat.messageCount)} 条 · {chat.chatKind === "group" ? "群聊" : "好友"}</small></span>
            </button>
          ))}
          {!chats.length && <p className="list-empty">没有匹配的聊天</p>}
        </div>
      </aside>

      <section className="detail-main">
        {loading ? <LoadingState /> : error ? <EmptyState title="无法读取聊天详情" description={error} /> : !detail ? (
          <EmptyState title="选择一个聊天" description="从左侧选择好友或群聊，查看聊天节奏与关键词。" />
        ) : (
          <>
            <header className="detail-hero panel">
              <div className={`avatar hero ${detail.chat.chatKind}`}>{detail.chat.displayName.slice(0, 1).toUpperCase()}</div>
              <div><p className="eyebrow">{detail.chat.chatKind === "group" ? "GROUP CHAT" : "DIRECT CHAT"}</p><h2>{detail.chat.displayName}</h2><p>{formatDateTime(detail.firstMessageAt)} — {formatDateTime(detail.lastMessageAt)}</p></div>
              <div className="detail-totals"><span><strong>{formatCount(detail.totalMessages)}</strong>条消息</span><span><strong>{detail.totalMessages ? Math.round((detail.sentMessages / detail.totalMessages) * 100) : 0}%</strong>由我发送</span></div>
            </header>

            <AiSummaryPanel accountId={accountId} chatId={detail.chat.id} chatName={detail.chat.displayName} range={range} revision={revision} />

            <div className="detail-charts">
              <section className="panel"><div className="panel-heading"><div><p className="eyebrow">TIMELINE</p><h2>聊天趋势</h2></div></div><Chart option={trendOption} height={240} ariaLabel="当前聊天消息趋势" /></section>
              <section className="panel"><div className="panel-heading"><div><p className="eyebrow">DAILY RHYTHM</p><h2>24 小时节奏</h2></div></div><Chart option={hourOption} height={240} ariaLabel="当前聊天小时活跃趋势" /></section>
            </div>

            <TopicsPanel accountId={accountId} chatId={detail.chat.id} chatName={detail.chat.displayName} range={range} revision={revision} keywords={detail.keywords} />

            <section className="panel timeline-panel">
              <div className="panel-heading"><div><p className="eyebrow">MESSAGES</p><h2>消息时间线</h2></div><span className="subtle">已显示 {detail.messages.length} 条</span></div>
              <div className="message-timeline">
                {detail.messages.map((message) => (
                  <article key={`${message.localId}-${message.serverId ?? ""}`} className={`${message.isFromMe ? "mine" : "theirs"}${isPreviewKind(message.kind) ? " has-media" : ""}`}>
                    <div className="message-meta"><strong>{message.isFromMe ? "我" : message.senderName || "对方"}</strong><time>{formatDateTime(message.occurredAt)}</time></div>
                    {isPreviewKind(message.kind)
                      ? <MediaBubble accountId={accountId} chatId={selectedId} message={message} onOpen={() => setViewerIndex(mediaMessages.indexOf(message))} />
                      : <p>{message.text ? withEmoji(message.text) : `[${messageKindLabel[message.kind] ?? message.kind}]`}</p>}
                  </article>
                ))}
              </div>
              {detail.nextOffset !== undefined && <button className="load-more" disabled={loadingMore} onClick={loadMore}>{loadingMore ? "正在加载…" : "加载更早消息"}</button>}
            </section>
          </>
        )}
      </section>
      {viewerIndex !== undefined && <Lightbox accountId={accountId} chatId={selectedId} messages={mediaMessages} index={viewerIndex} onIndexChange={setViewerIndex} onClose={() => setViewerIndex(undefined)} />}
    </div>
  );
}
