import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { clock, groupFootprint } from "../chatFootprint";
import type { DateRange } from "../types";
import { errorMessage, formatCount, rangeForPreset } from "../utils";
import { DateField } from "./DateField";
import { Icon } from "./Icons";
import { EmptyState, InlineNotice, LoadingState } from "./States";

type KindFilter = "all" | "group" | "direct";
type LoadStatus = "loading" | "ready" | "error";

interface ChatFootprintProps {
  accountId: string;
  range: DateRange;
  revision: number;
  onOpenChat: (chatId: string, localDate: string) => void;
  onCommitRange: (range: DateRange) => void;
}

export function ChatFootprint({ accountId, range, revision, onOpenChat, onCommitRange }: ChatFootprintProps) {
  const [kind, setKind] = useState<KindFilter>("all");
  const [status, setStatus] = useState<LoadStatus>("loading");
  const [error, setError] = useState("");
  const [entries, setEntries] = useState<Awaited<ReturnType<typeof api.chatFootprint>>>([]);
  const [openDates, setOpenDates] = useState<string[]>([]);
  const [attempt, setAttempt] = useState(0);
  const external = `${range.from ?? ""}|${range.to ?? ""}`;
  const [tracked, setTracked] = useState(external);
  const [bounds, setBounds] = useState<DateRange>({ from: range.from, to: range.to });
  if (tracked !== external) {
    setTracked(external);
    setBounds({ from: range.from, to: range.to });
  }
  const from = bounds.from ?? "";
  const to = bounds.to ?? "";
  const days = useMemo(() => groupFootprint(entries), [entries]);

  useEffect(() => {
    let active = true;
    setStatus("loading");
    setError("");
    api.chatFootprint(accountId, { from: from || undefined, to: to || undefined }, kind === "all" ? undefined : kind)
      .then((rows) => {
        if (!active) return;
        setEntries(rows);
        setOpenDates(rows[0] ? [rows[0].localDate] : []);
        setStatus("ready");
      })
      .catch((reason) => {
        if (!active) return;
        setEntries([]);
        setError(errorMessage(reason));
        setStatus("error");
      });
    return () => { active = false; };
  }, [accountId, revision, from, to, kind, attempt]);

  function toggle(localDate: string) {
    setOpenDates((current) => current.includes(localDate) ? current.filter((date) => date !== localDate) : [...current, localDate]);
  }

  function updateBound(edge: "from" | "to", value: string) {
    if (!value) return;
    const next = { ...bounds, [edge]: value };
    setBounds(next);
    onCommitRange(next);
  }

  return (
    <div className="footprint">
      <div className="footprint-intro panel">
        <span className="footprint-mark"><Icon name="calendar" /></span>
        <div>
          <h2>聊天足迹</h2>
          <p>按日期找回你聊过的群聊和私聊，点击即可回到当天的聊天记录。</p>
        </div>
        <span className="footprint-local">本地查询 · 不调用 AI</span>
      </div>

      <section className="panel footprint-filters">
        <div className="footprint-filter-row">
          <DateField label="开始日期" value={from} onChange={(value) => updateBound("from", value)} />
          <span>至</span>
          <DateField label="结束日期" value={to} onChange={(value) => updateBound("to", value)} />
          <div className="footprint-kinds" role="group" aria-label="聊天类型">
            {(["all", "group", "direct"] as KindFilter[]).map((value) => (
              <button key={value} type="button" className={kind === value ? "active" : ""} aria-pressed={kind === value} onClick={() => setKind(value)}>
                {value === "all" ? "全部" : value === "group" ? "群聊" : "私聊"}
              </button>
            ))}
          </div>
          {(from || to) && <button type="button" className="secondary-button" onClick={() => { setBounds({}); onCommitRange({}); }}>查看全部日期</button>}
        </div>
        <p>只显示已同步、且在所选日期里有消息的聊天。图片、视频和语音也会计数。</p>
      </section>

      {status === "loading" && <LoadingState label="正在读取聊天足迹…" />}
      {status === "error" && (
        <InlineNotice tone="error">
          <strong>聊天足迹没有加载成功</strong>
          <p>{error}</p>
          <button type="button" className="secondary-button notice-action" onClick={() => setAttempt((value) => value + 1)}>重试</button>
        </InlineNotice>
      )}
      {status === "ready" && days.length === 0 && (
        <EmptyState title="这个日期范围没有聊天记录" description="换一个日期范围，或确认聊天已经同步。" action={<button type="button" className="secondary-button" onClick={() => onCommitRange(rangeForPreset("30d"))}>重置为最近 30 天</button>} />
      )}
      {status === "ready" && days.length > 0 && (
        <>
          <div className="footprint-list-head">
            <div>
              <p className="eyebrow">DAILY CHAT HISTORY</p>
              <h2>每日聊天集合 <em>{days.length} 天</em></h2>
              <p>点击日期展开当天聊天，点击聊天回到那一天的消息。</p>
            </div>
            <span>日期 从新到旧</span>
          </div>
          <div className="footprint-days">
            {days.map((day) => {
              const open = openDates.includes(day.localDate);
              return (
                <section key={day.localDate} className="panel footprint-day">
                  <button type="button" className="footprint-day-head" aria-expanded={open} onClick={() => toggle(day.localDate)}>
                    <span className="footprint-datebox"><strong>{day.day}</strong><small>{day.month}</small></span>
                    <span className="footprint-day-title">
                      <strong>{day.localDate} · {day.weekday}</strong>
                      <small>{formatCount(day.chatCount)} 个聊天 · {formatCount(day.messageCount)} 条消息</small>
                    </span>
                    <span className="footprint-stat"><small>聊天</small><strong>{formatCount(day.chatCount)}</strong></span>
                    <span className="footprint-stat"><small>消息</small><strong>{formatCount(day.messageCount)}</strong></span>
                    <Icon name="chevron" className={open ? "footprint-chevron open" : "footprint-chevron"} />
                  </button>
                  {open && (
                    <div className="footprint-chats">
                      {day.chats.map((chat) => (
                        <button key={chat.chatId} type="button" className="footprint-chat" onClick={() => onOpenChat(chat.chatId, chat.localDate)}>
                          <span className={`avatar ${chat.chatKind}`}>{chat.displayName.slice(0, 1)}</span>
                          <span className="footprint-chat-copy">
                            <strong>{chat.displayName}<small>{chat.chatKind === "group" ? "群聊" : "私聊"}</small></strong>
                            <span>{clock(chat.firstMessageAt)} 首条 · {clock(chat.lastMessageAt)} 末条</span>
                          </span>
                          <b>{formatCount(chat.messageCount)} 条</b>
                          <span className="footprint-times">{clock(chat.firstMessageAt)}–{clock(chat.lastMessageAt)}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </section>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
