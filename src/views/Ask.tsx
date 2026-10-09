import { Fragment, useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { api } from "../api";
import { runAiTask, stopAiTask, useAi, useAiTask, useSessionState } from "../ai";
import { withEmoji } from "../emoji";
import type { AiIndexStatus, AskResult, AskSource, ChatListItem, DateRange, SearchHit } from "../types";
import { Icon } from "../components/Icons";
import { Markdown } from "../components/Markdown";
import { MessageContext } from "../components/MessageContext";
import { EmptyState, InlineNotice, LoadingState } from "../components/States";
import { errorMessage, formatDateTime } from "../utils";

interface AskProps {
  accountId: string;
  range: DateRange;
  revision: number;
  onOpenDetail: (chatId: string) => void;
  onOpenSettings: () => void;
}

type Mode = "ask" | "search";

interface ContextTarget {
  chatId: string;
  chatName: string;
  localId: number;
  endLocalId?: number;
}

const EXAMPLES = ["最近有人推荐过好吃的餐厅吗？", "上次说要一起旅行是去哪里？", "我答应过别人要做什么事？"];

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

export function Ask({ accountId, range, revision, onOpenDetail, onOpenSettings }: AskProps) {
  const { settings, confirmCloud } = useAi();
  const [pickedMode, setMode] = useSessionState<Mode | undefined>(`ask-mode:${accountId}`, undefined);
  const mode = pickedMode ?? (settings.enabled ? "ask" : "search");
  const [query, setQuery] = useSessionState(`ask-query:${accountId}`, "");
  const [chatId, setChatId] = useSessionState(`ask-chat:${accountId}`, "");
  const [chats, setChats] = useState<ChatListItem[]>([]);
  const [indexStatus, setIndexStatus] = useState<AiIndexStatus>();
  const [hits, setHits] = useSessionState<SearchHit[] | undefined>(`ask-hits:${accountId}`, undefined);
  const [searchedFor, setSearchedFor] = useSessionState(`ask-searched:${accountId}`, "");
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState("");
  const [context, setContext] = useState<ContextTarget>();
  const askKey = `ask:${accountId}`;
  const stream = useAiTask<AskResult>(askKey);
  const answer = stream.result;
  const asked = stream.label;
  const error = mode === "ask" ? stream.error : searchError;

  useEffect(() => {
    api.chats(accountId).then(setChats).catch(() => setChats([]));
  }, [accountId, revision]);

  useEffect(() => {
    if (!settings.enabled) return;
    api.aiIndexStatus(accountId).then(setIndexStatus).catch(() => setIndexStatus(undefined));
  }, [accountId, settings.enabled, settings.embedModel, revision]);

  const sourcesByIndex = useMemo(() => new Map((answer?.sources ?? []).map((source) => [source.index, source])), [answer]);

  function openSource(source: AskSource) {
    setContext({ chatId: source.chatId, chatName: source.chatName, localId: source.localId, endLocalId: source.endLocalId });
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
      title: "发送问题和相关聊天片段",
      detail: `你的问题，以及从${scope}检索到的最多 12 段相关聊天片段（每段不超过约 600 字）。`,
    });
    if (consent === null) return;
    await runAiTask(askKey, (requestId) => api.ask(accountId, text, range, chatId, requestId, consent), { reset: true, label: text });
  }

  function submit(event?: FormEvent, value = query) {
    event?.preventDefault();
    const text = value.trim();
    if (!text || stream.running || searching) return;
    if (mode === "search") void runSearch(text);
    else void runAsk(text);
  }

  const aiReady = settings.enabled;
  const needsIndex = aiReady && indexStatus && indexStatus.indexedChunks === 0;

  return (
    <div className="view-content ask-view">
      <section className="panel ask-panel">
        <div className="segmented ask-modes" role="tablist" aria-label="搜索方式">
          <button type="button" role="tab" aria-selected={mode === "ask"} className={mode === "ask" ? "active" : ""} onClick={() => setMode("ask")}><Icon name="sparkle" />AI 问答</button>
          <button type="button" role="tab" aria-selected={mode === "search"} className={mode === "search" ? "active" : ""} onClick={() => setMode("search")}><Icon name="search" />关键词搜索</button>
        </div>
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
        <p className="subtle ask-hint">{mode === "ask" ? "回答只依据检索到的聊天片段，并标注出处；生成期间可以切换到其他页面；" : "关键词搜索完全在本机完成；"}结果受顶部日期范围限制。</p>
      </section>

      {mode === "ask" && !aiReady && (
        <InlineNotice tone="warning">
          <strong>AI 问答尚未开启</strong>
          <p>在“数据管理 → AI 设置”中开启，并连接本机的 Ollama 或其他模型服务。关键词搜索无需开启 AI。</p>
          <button type="button" className="secondary-button notice-action" onClick={onOpenSettings}>前往 AI 设置</button>
        </InlineNotice>
      )}
      {mode === "ask" && needsIndex && (
        <InlineNotice tone="info">
          <strong>还没有建立语义索引</strong>
          <p>现在只能按关键词检索相关聊天。建立索引后，用不同说法提问也能找到内容。</p>
          <button type="button" className="secondary-button notice-action" onClick={onOpenSettings}>去建立索引</button>
        </InlineNotice>
      )}
      {error && <InlineNotice tone="error">{error}</InlineNotice>}

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
      ) : searching ? <LoadingState label="正在搜索…" /> : hits && (
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
      )}

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
