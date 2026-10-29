import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { formatChars, runAiTask, stopAiTask, useAi, useAiTask, usesCloud } from "../ai";
import type { Commitment, CommitmentScanOutcome, DateRange, Decision, DecisionStatus } from "../types";
import { directionLabel, dueLabel, followupLabel, isOverdue } from "../commitments";
import { errorMessage, formatDateTime, toLocalDate } from "../utils";
import { Icon } from "./Icons";
import { MemoryCard } from "./MemoryCard";
import { MessageContext } from "./MessageContext";
import { InlineNotice } from "./States";

interface ChatMemoryPanelProps {
  accountId: string;
  chatId: string;
  chatName: string;
  chatKind: string;
  range: DateRange;
  revision: number;
  onOpenDetail: (chatId: string) => void;
  onOpenCommitments: () => void;
}

type DecisionTab = "suggested" | "decision" | "question" | "closed";

const DECISION_HINT: Record<string, string> = {
  overturned: "后续消息显示这条决定可能已被推翻",
  changed: "后续消息显示内容可能有变",
  resolved: "后续消息显示这件事可能已经定下来",
};

interface ContextTarget {
  localId: number;
}

interface DecisionDraft {
  id: number;
  title: string;
  owner: string;
  dueDate: string;
}

function matchesTab(item: Decision, tab: DecisionTab): boolean {
  if (tab === "suggested") return item.status === "suggested";
  if (tab === "closed") return item.status === "closed" || item.status === "dismissed";
  return item.status === "active" && item.kind === tab;
}

export function ChatMemoryPanel({ accountId, chatId, chatName, chatKind, range, revision, onOpenDetail, onOpenCommitments }: ChatMemoryPanelProps) {
  const { settings, confirmCloud } = useAi();
  const isGroup = chatKind === "group";
  const [commitments, setCommitments] = useState<Commitment[]>([]);
  const [decisions, setDecisions] = useState<Decision[]>([]);
  const [tab, setTab] = useState<DecisionTab>("suggested");
  const [error, setError] = useState("");
  const [draft, setDraft] = useState<DecisionDraft>();
  const [context, setContext] = useState<ContextTarget>();
  const commitmentKey = `commitments:${accountId}`;
  const decisionKey = `decisions:${accountId}:${chatId}`;
  const commitmentScan = useAiTask<CommitmentScanOutcome>(commitmentKey);
  const decisionScan = useAiTask<CommitmentScanOutcome>(decisionKey);
  const today = toLocalDate(new Date());

  const reload = useCallback(async () => {
    try {
      const [promises, records] = await Promise.all([
        api.commitments(accountId, { chatId, statuses: ["suggested", "open"] }),
        isGroup ? api.decisions(accountId, { chatId }) : Promise.resolve([]),
      ]);
      setCommitments(promises);
      setDecisions(records);
      setError("");
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [accountId, chatId, isGroup]);

  useEffect(() => { void reload(); }, [reload, revision, commitmentScan.running, decisionScan.running]);

  async function scan(kind: "commitment" | "decision") {
    const running = kind === "commitment" ? commitmentScan.running : decisionScan.running;
    if (running) return;
    let detail = kind === "commitment"
      ? `“${chatName}”在当前日期范围内含有约定线索的聊天，以及这里未完成的约定。`
      : `“${chatName}”在当前日期范围内含有决策线索的聊天，以及这个群里仍然有效的决定和待定事项。`;
    if (usesCloud(settings)) {
      try {
        const estimate = kind === "commitment"
          ? await api.commitmentEstimate(accountId, range, chatId)
          : await api.decisionEstimate(accountId, chatId, range);
        detail = `${detail}大约 ${estimate.windows.toLocaleString("zh-CN")} 段、${formatChars(estimate.chars)}。`;
      } catch (reason) {
        setError(errorMessage(reason));
        return;
      }
    }
    const consent = await confirmCloud({ title: kind === "commitment" ? "发送聊天以找出约定" : "发送群聊以找出决策", detail });
    if (consent === null) return;
    const key = kind === "commitment" ? commitmentKey : decisionKey;
    await runAiTask(key, (requestId) => kind === "commitment"
      ? api.scanCommitments(accountId, range, chatId, requestId, consent)
      : api.scanDecisions(accountId, chatId, range, requestId, consent), { reset: true });
  }

  async function updateDecision(id: number, patch: { status?: DecisionStatus; edit?: DecisionDraft }) {
    try {
      await api.updateDecision(accountId, id, {
        status: patch.status,
        edit: patch.edit ? { title: patch.edit.title, owner: patch.edit.owner, dueDate: patch.edit.dueDate } : undefined,
      });
      setDraft(undefined);
      await reload();
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }

  const shown = decisions.filter((item) => matchesTab(item, tab));
  const count = (value: DecisionTab) => decisions.filter((item) => matchesTab(item, value)).length;

  return (
    <section className="panel chat-memory">
      <div className="panel-heading">
        <div><p className="eyebrow">PROMISES & DECISIONS</p><h2>约定与决策</h2></div>
        <div className="memory-actions">
          {settings.enabled && (commitmentScan.running
            ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(commitmentKey)}><Icon name="stop" />暂停约定扫描</button>
            : <button type="button" className="secondary-button compact" onClick={() => scan("commitment")}><Icon name="sparkle" />扫描约定</button>)}
          {isGroup && settings.enabled && (decisionScan.running
            ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(decisionKey)}><Icon name="stop" />暂停决策扫描</button>
            : <button type="button" className="primary-button compact" onClick={() => scan("decision")}><Icon name="sparkle" />扫描决策</button>)}
        </div>
      </div>
      <div className="chat-memory-body">
        {error && <InlineNotice tone="error">{error}</InlineNotice>}
        {(commitmentScan.running || decisionScan.running) && <p className="stream-status" role="status"><span className="spinner" />{commitmentScan.status || decisionScan.status || "正在扫描…"}</p>}
        {!commitmentScan.running && commitmentScan.error && <p className="ai-error">{commitmentScan.error}</p>}
        {!decisionScan.running && decisionScan.error && <p className="ai-error">{decisionScan.error}</p>}

        <div className="memory-block">
          <div className="memory-block-head"><h3>这个聊天的约定</h3><button type="button" className="link-button" onClick={onOpenCommitments}>在约定与待办中查看全部</button></div>
          {commitments.length ? (
            <ol className="commitment-list compact">
              {commitments.map((item) => (
                <MemoryCard
                  key={item.id}
                  overdue={isOverdue(item, today)}
                  tags={<><span className={`direction-tag ${item.direction}`}>{directionLabel(item)}</span>{item.dueDate && <span className="due-tag"><Icon name="calendar" />{dueLabel(item.dueDate, today)}</span>}{item.status === "suggested" && <span className="status-tag">待确认</span>}</>}
                  title={item.title}
                  meta={formatDateTime(item.sourceAt)}
                  quote={item.quote}
                  onQuote={() => setContext({ localId: item.sourceLocalId })}
                  hint={item.aiStatus && followupLabel[item.aiStatus] ? <p className="commitment-hint"><Icon name="sparkle" />{followupLabel[item.aiStatus]}</p> : undefined}
                />
              ))}
            </ol>
          ) : <p className="ai-panel-hint">这里还没有待确认或进行中的约定。</p>}
        </div>

        {isGroup && (
          <div className="memory-block">
            <div className="memory-block-head"><h3>群聊决策</h3></div>
            <div className="segmented" role="tablist" aria-label="决策状态">
              {(["suggested", "decision", "question", "closed"] as DecisionTab[]).map((value) => (
                <button key={value} type="button" role="tab" aria-selected={tab === value} className={tab === value ? "active" : ""} onClick={() => setTab(value)}>
                  {value === "suggested" ? "待确认" : value === "decision" ? "已决定" : value === "question" ? "待定" : "已推翻或已解决"}
                  {count(value) ? <span className="tab-count">{count(value)}</span> : null}
                </button>
              ))}
            </div>
            {shown.length ? (
              <ol className="commitment-list compact">
                {shown.map((item) => {
                  const editing = draft?.id === item.id;
                  const closeLabel = item.kind === "question" ? "标记已解决" : "标记推翻";
                  return (
                    <MemoryCard
                      key={item.id}
                      tags={<>
                        <span className={`direction-tag ${item.kind === "question" ? "theirs" : "mine"}`}>{item.kind === "question" ? "待定" : "已决定"}</span>
                        {item.ownerName && <span className="due-tag">{item.ownerName}负责</span>}
                        {item.dueDate && <span className="due-tag"><Icon name="calendar" />{item.dueDate}</span>}
                        {item.status === "dismissed" && <span className="status-tag">已忽略</span>}
                        {item.status === "closed" && <span className="status-tag">{item.kind === "question" ? "已解决" : "已推翻"}</span>}
                      </>}
                      title={item.title}
                      editing={editing ? (
                        <form className="commitment-edit" onSubmit={(event) => { event.preventDefault(); void updateDecision(item.id, { edit: draft }); }}>
                          <input value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} aria-label="决策内容" maxLength={60} autoFocus />
                          <input value={draft.owner} onChange={(event) => setDraft({ ...draft, owner: event.target.value })} aria-label="负责人" placeholder="负责人" maxLength={20} />
                          <input type="date" value={draft.dueDate} onChange={(event) => setDraft({ ...draft, dueDate: event.target.value })} aria-label="截止日期" />
                          <button type="submit" className="primary-button compact">保存</button>
                          <button type="button" className="secondary-button compact" onClick={() => setDraft(undefined)}>取消</button>
                        </form>
                      ) : undefined}
                      meta={`${item.speakerName} · ${formatDateTime(item.sourceAt)}`}
                      quote={item.quote}
                      onQuote={() => setContext({ localId: item.sourceLocalId })}
                      hint={item.aiStatus && DECISION_HINT[item.aiStatus] ? (
                        <p className="commitment-hint"><Icon name="sparkle" />{DECISION_HINT[item.aiStatus]}{item.aiNote ? `：${item.aiNote}` : ""}</p>
                      ) : undefined}
                      primary={item.status === "suggested" ? { label: "确认", onClick: () => updateDecision(item.id, { status: "active" }) }
                        : item.status === "active" ? { label: closeLabel, onClick: () => updateDecision(item.id, { status: "closed" }), tone: "secondary" }
                        : { label: "恢复", onClick: () => updateDecision(item.id, { status: "active" }), tone: "secondary" }}
                      menu={item.status === "closed" || item.status === "dismissed" ? undefined : [
                        { label: "编辑", onClick: () => setDraft({ id: item.id, title: item.title, owner: item.ownerName ?? "", dueDate: item.dueDate ?? "" }) },
                        { label: "忽略", onClick: () => updateDecision(item.id, { status: "dismissed" }) },
                      ]}
                    />
                  );
                })}
              </ol>
            ) : <p className="ai-panel-hint">{settings.enabled ? "扫描后，确认过的结论会留在这里。" : "开启 AI 后可以扫描这个群的决策。已经保存的记录不需要 AI 也能查看。"}</p>}
          </div>
        )}
      </div>
      {context && (
        <MessageContext
          accountId={accountId}
          chatId={chatId}
          chatName={chatName}
          localId={context.localId}
          onOpenDetail={onOpenDetail}
          onClose={() => setContext(undefined)}
        />
      )}
    </section>
  );
}
