import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { formatChars, runAiTask, stopAiTask, useAi, useAiTask, usesCloud, useSessionState } from "../ai";
import { directionLabel, dueLabel, followupLabel, isOverdue, scanSummary, sortOpen } from "../commitments";
import type { ChatListItem, Commitment, CommitmentCounts, CommitmentDirection, CommitmentPatch, CommitmentScanOutcome, CommitmentStatus, DateRange } from "../types";
import { Icon } from "../components/Icons";
import { MemoryCard } from "../components/MemoryCard";
import { MessageContext } from "../components/MessageContext";
import { EmptyState, InlineNotice, LoadingState } from "../components/States";
import { errorMessage, formatDateTime, toLocalDate } from "../utils";

interface CommitmentsProps {
  accountId: string;
  range: DateRange;
  revision: number;
  onCountsChange: (counts: CommitmentCounts) => void;
  onOpenDetail: (chatId: string) => void;
  onOpenSettings: () => void;
}

type Tab = "suggested" | "open" | "closed";

const TAB_STATUSES: Record<Tab, CommitmentStatus[]> = {
  suggested: ["suggested"],
  open: ["open"],
  closed: ["done", "dismissed"],
};

const EMPTY: Record<Tab, { title: string; description: string }> = {
  suggested: { title: "没有待确认的约定", description: "扫描聊天后，AI 找到的约定会先放在这里，由你确认或忽略。" },
  open: { title: "没有进行中的约定", description: "确认过的约定会出现在这里，按截止日期排列。" },
  closed: { title: "还没有完成或忽略的约定", description: "标记完成或忽略后，约定会移到这里，随时可以恢复。" },
};

interface ContextTarget {
  chatId: string;
  chatName: string;
  localId: number;
}

interface Draft {
  id: number;
  title: string;
  dueDate: string;
}

export function Commitments({ accountId, range, revision, onCountsChange, onOpenDetail, onOpenSettings }: CommitmentsProps) {
  const { settings, confirmCloud } = useAi();
  const [tab, setTab] = useSessionState<Tab>(`commitments-tab:${accountId}`, "suggested");
  const [direction, setDirection] = useSessionState<CommitmentDirection | "">(`commitments-direction:${accountId}`, "");
  const [chatId, setChatId] = useSessionState(`commitments-chat:${accountId}`, "");
  const [chats, setChats] = useState<ChatListItem[]>([]);
  const [items, setItems] = useState<Commitment[]>();
  const [counts, setCounts] = useState<CommitmentCounts>();
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<Draft>();
  const [context, setContext] = useState<ContextTarget>();
  const scanKey = `commitments:${accountId}`;
  const scan = useAiTask<CommitmentScanOutcome>(scanKey);
  const today = toLocalDate(new Date());

  useEffect(() => {
    api.chats(accountId).then(setChats).catch(() => setChats([]));
  }, [accountId, revision]);

  const reload = useCallback(async () => {
    try {
      const [values, totals] = await Promise.all([
        api.commitments(accountId, { statuses: TAB_STATUSES[tab], direction: direction || undefined }),
        api.commitmentCounts(accountId),
      ]);
      setItems(tab === "open" ? sortOpen(values) : tab === "closed" ? [...values].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)) : values);
      setCounts(totals);
      onCountsChange(totals);
      setError("");
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [accountId, tab, direction, onCountsChange]);

  useEffect(() => { void reload(); }, [reload, revision, scan.running]);

  async function startScan() {
    const scope = chatId ? `“${chats.find((chat) => chat.id === chatId)?.displayName ?? "所选聊天"}”` : "所有聊天";
    let detail = `${scope}在当前日期范围内含有约定线索的聊天，以及未完成约定的标题。`;
    if (usesCloud(settings)) {
      try {
        const estimate = await api.commitmentEstimate(accountId, range, chatId);
        detail = `${scope}中大约 ${estimate.windows.toLocaleString("zh-CN")} 段聊天（来自 ${estimate.chats} 个聊天，约 ${formatChars(estimate.chars)}），以及未完成约定的标题。`;
      } catch (reason) {
        setError(errorMessage(reason));
        return;
      }
    }
    const consent = await confirmCloud({ title: "发送聊天以找出约定", detail });
    if (consent === null) return;
    await runAiTask(scanKey, (requestId) => api.scanCommitments(accountId, range, chatId, requestId, consent), { reset: true });
  }

  async function update(id: number, patch: CommitmentPatch) {
    try {
      await api.updateCommitment(accountId, id, patch);
      setDraft(undefined);
      await reload();
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }

  const tabCount = (value: Tab) => !counts ? undefined : value === "suggested" ? counts.suggested : value === "open" ? counts.open : counts.done + counts.dismissed;

  return (
    <div className="view-content commitments-view">
      <section className="panel commitment-scan">
        <div className="panel-heading">
          <div><p className="eyebrow">COMMITMENT RADAR</p><h2>从聊天里找出约定</h2></div>
          {settings.enabled && (scan.running
            ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(scanKey)}><Icon name="stop" />暂停</button>
            : <button type="button" className="primary-button compact" onClick={startScan}><Icon name="sparkle" />扫描聊天</button>)}
        </div>
        <div className="commitment-scan-body">
          {settings.enabled ? (
            <>
              <div className="commitment-scan-row">
                <p className="ai-panel-hint">从当前日期范围找出答应过的事，先放进「待确认」。</p>
                <select value={chatId} onChange={(event) => setChatId(event.target.value)} aria-label="扫描范围" disabled={scan.running}>
                  <option value="">全部聊天</option>
                  {chats.map((chat) => <option key={chat.id} value={chat.id}>{chat.displayName}</option>)}
                </select>
              </div>
              {!range.from && !scan.running && <p className="subtle">当前日期范围是全部记录，第一次扫描可能需要较长时间。建议先在右上角选「近 30 天」。</p>}
              {scan.running && <p className="stream-status" role="status"><span className="spinner" />{scan.status || "正在准备扫描…"}</p>}
              {!scan.running && scan.result && <p className="ai-meta" role="status">{scanSummary(scan.result)}</p>}
              {scan.error && <p className="ai-error">{scan.error}</p>}
            </>
          ) : (
            <InlineNotice tone="warning">
              <strong>扫描需要先开启 AI</strong>
              <p>到「数据管理 → AI 设置」里开启。已经保存的约定仍可查看。</p>
              <button type="button" className="secondary-button notice-action" onClick={onOpenSettings}>前往 AI 设置</button>
            </InlineNotice>
          )}
          {!!counts?.suggestedTotal && <p className="subtle commitment-stats">AI 一共建议了 {counts.suggestedTotal} 条约定，你保留了 {counts.acceptedTotal} 条{counts.overdue ? `；${counts.overdue} 条已过期` : ""}。</p>}
        </div>
      </section>

      <section className="panel commitment-list-panel">
        <div className="commitment-toolbar">
          <div className="segmented" role="tablist" aria-label="约定状态">
            {(["suggested", "open", "closed"] as Tab[]).map((value) => (
              <button key={value} type="button" role="tab" aria-selected={tab === value} className={tab === value ? "active" : ""} onClick={() => setTab(value)}>
                {value === "suggested" ? "待确认" : value === "open" ? "进行中" : "已完成或忽略"}
                {tabCount(value) ? <span className="tab-count">{tabCount(value)}</span> : null}
              </button>
            ))}
          </div>
          <div className="segmented" role="group" aria-label="谁答应的">
            {([["", "全部"], ["mine", "我答应的"], ["theirs", "对方答应我的"]] as const).map(([value, label]) => (
              <button key={value || "all"} type="button" aria-pressed={direction === value} className={direction === value ? "active" : ""} onClick={() => setDirection(value)}>{label}</button>
            ))}
          </div>
        </div>
        {error && <InlineNotice tone="error">{error}</InlineNotice>}
        {!items ? <LoadingState label="正在读取约定…" /> : !items.length ? <EmptyState {...EMPTY[tab]} /> : (
          <ol className="commitment-list">
            {items.map((item) => {
              const overdue = isOverdue(item, today);
              const hint = (item.status === "suggested" || item.status === "open") && item.aiStatus ? followupLabel[item.aiStatus] : undefined;
              const editing = draft?.id === item.id;
              const followup = item.followupLocalId;
              const openItem = item.status === "suggested" || item.status === "open";
              return (
                <MemoryCard
                  key={item.id}
                  overdue={overdue}
                  tags={<>
                    <span className={`direction-tag ${item.direction}`}>{directionLabel(item)}</span>
                    {item.dueDate
                      ? <span className={`due-tag${overdue ? " overdue" : ""}`}><Icon name="calendar" />{item.dueDate}{openItem ? ` · ${dueLabel(item.dueDate, today)}` : ""}</span>
                      : item.dueText && <span className="due-tag"><Icon name="calendar" />{item.dueText}</span>}
                    {item.status === "done" && <span className="status-tag done">已完成</span>}
                    {item.status === "dismissed" && <span className="status-tag">已忽略</span>}
                  </>}
                  title={item.title}
                  editing={editing ? (
                    <form className="commitment-edit" onSubmit={(event) => { event.preventDefault(); void update(item.id, { edit: { title: draft.title, dueDate: draft.dueDate } }); }}>
                      <input value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} aria-label="约定内容" maxLength={60} autoFocus />
                      <input type="date" value={draft.dueDate} onChange={(event) => setDraft({ ...draft, dueDate: event.target.value })} aria-label="截止日期" />
                      <button type="submit" className="primary-button compact">保存</button>
                      <button type="button" className="secondary-button compact" onClick={() => setDraft(undefined)}>取消</button>
                    </form>
                  ) : undefined}
                  meta={`${item.chatName} · ${formatDateTime(item.sourceAt)}`}
                  quote={item.quote}
                  onQuote={() => setContext({ chatId: item.chatId, chatName: item.chatName, localId: item.sourceLocalId })}
                  hint={hint ? (
                    <p className="commitment-hint">
                      <Icon name="sparkle" />{hint}{item.aiNote ? `：${item.aiNote}` : ""}
                      {followup != null && <button type="button" className="link-button" onClick={() => setContext({ chatId: item.chatId, chatName: item.chatName, localId: followup })}>查看后续</button>}
                    </p>
                  ) : undefined}
                  primary={item.status === "suggested" ? { label: "确认", onClick: () => update(item.id, { status: "open" }) }
                    : item.status === "open" ? { label: "完成", onClick: () => update(item.id, { status: "done" }) }
                    : { label: "恢复", onClick: () => update(item.id, { status: "open" }), tone: "secondary" }}
                  menu={openItem ? [
                    { label: "编辑", onClick: () => setDraft({ id: item.id, title: item.title, dueDate: item.dueDate ?? "" }) },
                    { label: "忽略", onClick: () => update(item.id, { status: "dismissed" }) },
                  ] : undefined}
                />
              );
            })}
          </ol>
        )}
      </section>

      {context && (
        <MessageContext
          accountId={accountId}
          chatId={context.chatId}
          chatName={context.chatName}
          localId={context.localId}
          onOpenDetail={(id) => { setContext(undefined); onOpenDetail(id); }}
          onClose={() => setContext(undefined)}
        />
      )}
    </div>
  );
}
