import { useEffect, useMemo, useState } from "react";
import type { EChartsOption } from "echarts";
import { api } from "../api";
import { getAiTask, newRequestId, runAiTask, setAiTaskResult, stopAiTask, useAi, useAiTask, useSessionState, usesCloud } from "../ai";
import type { RelationshipInsight, ReportResult, YearInsights } from "../types";
import { Chart } from "../components/Chart";
import { Icon } from "../components/Icons";
import { Markdown } from "../components/Markdown";
import { EmptyState, LoadingState } from "../components/States";
import { chartTheme } from "../theme";
import { errorMessage, formatCount, formatDateTime } from "../utils";

interface ReportProps {
  accountId: string;
  revision: number;
  onOpenDetail: (chatId: string) => void;
  onOpenSettings: () => void;
}

const trendLabels: Record<RelationshipInsight["trend"], string> = {
  new: "今年开始",
  warming: "升温",
  cooling: "降温",
  steady: "平稳",
};

function formatMinutes(value?: number | null): string {
  if (value == null) return "—";
  if (value < 1) return "不到 1 分钟";
  if (value < 60) return `${Math.round(value)} 分钟`;
  return `${(value / 60).toFixed(1)} 小时`;
}

function changeText(current: number, previous: number): string {
  if (!previous) return "去年没有记录";
  const ratio = (current - previous) / previous;
  if (Math.abs(ratio) < 0.005) return "与去年持平";
  return `比去年${ratio > 0 ? "多" : "少"} ${Math.abs(Math.round(ratio * 100))}%`;
}

function clock(value: string): string {
  return value.slice(11, 16);
}

export function Report({ accountId, revision, onOpenDetail, onOpenSettings }: ReportProps) {
  const { settings } = useAi();
  const [year, setYear] = useSessionState<string | undefined>(`report-year:${accountId}`, undefined);
  const [insights, setInsights] = useState<YearInsights>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [anonymize, setAnonymize] = useSessionState("report-anonymize", true);
  const cloud = usesCloud(settings);
  const reportKey = `report:${accountId}:${insights?.year ?? ""}`;
  const { text, status, running, result: report, error: reportError } = useAiTask<ReportResult>(reportKey);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    api.yearInsights(accountId, year)
      .then((value) => { if (active) { setInsights(value); if (!year) setYear(value.year); } })
      .catch((reason) => active && setError(errorMessage(reason)))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [accountId, year, revision, setYear]);

  useEffect(() => {
    const currentYear = insights?.year;
    if (!settings.enabled || !currentYear || getAiTask(reportKey).running) return;
    let active = true;
    api.yearReport(accountId, currentYear, newRequestId(), "cached", cloud && anonymize)
      .then((value) => { if (active) setAiTaskResult(reportKey, value ?? undefined); })
      .catch(() => undefined);
    return () => { active = false; };
  }, [reportKey, accountId, insights?.year, revision, settings.enabled, settings.chatModel, cloud, anonymize]);

  async function generate() {
    if (!insights) return;
    const targetYear = insights.year;
    await runAiTask(reportKey, (requestId) => api.yearReport(accountId, targetYear, requestId, report ? "refresh" : "generate", cloud && anonymize));
  }

  const monthlyOption = useMemo<EChartsOption>(() => {
    const now = new Date();
    // Months that have not happened yet would only show as empty bars.
    const months = (insights?.monthly ?? []).filter((item) => item.bucket <= `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`);
    return {
      tooltip: {
        ...chartTheme.tooltip,
        trigger: "axis",
        axisPointer: { type: "shadow", shadowStyle: { color: "rgba(244, 87, 58, .06)" } },
        valueFormatter: (value) => `${Number(value).toLocaleString("zh-CN")} 条`,
      },
      grid: { top: 18, left: 12, right: 16, bottom: 14, containLabel: true },
      xAxis: { type: "category", data: months.map((item) => `${Number(item.bucket.slice(5))}月`), axisTick: { show: false }, axisLabel: { color: chartTheme.axis }, axisLine: { lineStyle: { color: chartTheme.grid } } },
      yAxis: { type: "value", minInterval: 1, splitNumber: 4, axisLabel: { color: chartTheme.axis, formatter: (value: number) => formatCount(value) }, splitLine: { lineStyle: { color: chartTheme.grid } } },
      series: [{ type: "bar", data: months.map((item) => item.count), barMaxWidth: 32, itemStyle: { color: chartTheme.barGradient, borderRadius: [6, 6, 0, 0] } }],
    };
  }, [insights]);

  if (loading && !insights) return <LoadingState />;
  if (error) return <EmptyState title="暂时无法生成年度统计" description={error} />;
  if (!insights || !insights.availableYears.length) return <EmptyState title="还没有可分析的消息" description="完成一次微信同步，或从其他设备导入 Chat Insight 数据。" />;

  const cards = [
    { label: "全年消息", value: formatCount(insights.totalMessages), meta: changeText(insights.totalMessages, insights.previousTotal), icon: "message" as const },
    { label: "有聊天的日子", value: `${insights.activeDays} 天`, meta: `和 ${insights.chatCount} 个好友或群聊`, icon: "calendar" as const },
    { label: "最长连续聊天", value: insights.longestStreak ? `${insights.longestStreak.days} 天` : "—", meta: insights.longestStreak ? `${insights.longestStreak.start.slice(5)} 至 ${insights.longestStreak.end.slice(5)}` : "没有记录", icon: "users" as const },
    { label: "最晚的一次发消息", value: insights.latestNight ? clock(insights.latestNight.occurredAt) : "—", meta: insights.latestNight ? `${insights.latestNight.occurredAt.slice(5, 10)} · ${insights.latestNight.chatName}` : "今年作息很规律", icon: "send" as const },
  ];

  const aiBody = !settings.enabled ? (
    <div className="ai-panel-hint">
      <p>开启 AI 后，可以根据这一年的统计数据写一份年度聊天回顾。只发送统计数字，不发送任何聊天内容。</p>
      <button type="button" className="secondary-button compact" onClick={onOpenSettings}>前往 AI 设置</button>
    </div>
  ) : running ? (
    text ? <><Markdown text={text} /><p className="stream-status"><span className="spinner" />{status || "正在撰写…"}</p></> : <LoadingState label={status || "正在准备…"} />
  ) : report ? (
    <>
      <Markdown text={report.text} />
      <p className="ai-meta">{report.anonymized ? "联系人名称以代号发送给模型 · " : ""}{report.generatedAt ? `生成于 ${formatDateTime(report.generatedAt)}` : ""}</p>
    </>
  ) : text ? (
    <><Markdown text={text} /><p className="ai-meta">已停止生成</p></>
  ) : <p className="ai-panel-hint">AI 只会读取本页的统计数字（消息数、时段、聊天对象排行等），不会读取任何聊天内容。生成期间可以切换到其他页面。</p>;

  return (
    <div className="view-content report-view">
      <div className="report-years segmented" role="tablist" aria-label="选择年份">
        {insights.availableYears.map((value) => (
          <button key={value} type="button" role="tab" aria-selected={value === insights.year} className={value === insights.year ? "active" : ""} onClick={() => setYear(value)}>{value}</button>
        ))}
      </div>

      <section className="report-hero">
        <p className="eyebrow">{insights.year} IN CHATS</p>
        <h2>{insights.year} 年，你在微信里留下了 <em>{insights.totalMessages.toLocaleString("zh-CN")}</em> 条消息</h2>
        <p>其中 {insights.sentMessages.toLocaleString("zh-CN")} 条由你发出。{insights.busiestDay && `最热闹的一天是 ${insights.busiestDay.bucket}，共 ${insights.busiestDay.count} 条消息。`}{insights.busiestHour && `你最常在 ${insights.busiestHour.hour}:00 前后聊天。`}</p>
      </section>

      <div className="metric-grid">
        {cards.map((card) => (
          <article className="metric-card" key={card.label}>
            <div className="metric-icon"><Icon name={card.icon} /></div>
            <div><span>{card.label}</span><strong>{card.value}</strong><small>{card.meta}</small></div>
          </article>
        ))}
      </div>

      <section className="panel ai-panel report-ai" aria-live="polite">
        <div className="panel-heading">
          <div><p className="eyebrow">AI REVIEW</p><h2>年度聊天回顾</h2></div>
          {settings.enabled && (
            <div className="panel-actions">
              {cloud && <label className="checkbox-label"><input type="checkbox" checked={anonymize} disabled={running} onChange={(event) => setAnonymize(event.target.checked)} />用代号替换联系人名称</label>}
              {running
                ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(reportKey)}><Icon name="stop" />停止</button>
                : <button type="button" className={report ? "secondary-button compact" : "primary-button compact"} onClick={generate}><Icon name="sparkle" />{report ? "重新生成" : "生成回顾"}</button>}
            </div>
          )}
        </div>
        <div className="ai-panel-body">
          {aiBody}
          {reportError && <p className="ai-error">{reportError}</p>}
        </div>
      </section>

      <div className="dashboard-grid">
        <section className="panel span-2 chart-panel">
          <div className="panel-heading"><div><p className="eyebrow">MONTH BY MONTH</p><h2>每月消息</h2></div>{insights.busiestMonth && <span className="subtle">最多：{Number(insights.busiestMonth.bucket.slice(5))} 月</span>}</div>
          <Chart option={monthlyOption} fill ariaLabel="每月消息数量" />
        </section>
        <section className="panel">
          <div className="panel-heading"><div><p className="eyebrow">TOP CHATS</p><h2>年度聊天排行</h2></div></div>
          <ol className="ranking-list">
            {insights.topChats.slice(0, 6).map((chat, index) => (
              <li key={chat.chatId}>
                <button type="button" onClick={() => onOpenDetail(chat.chatId)}>
                  <span className="rank">{String(index + 1).padStart(2, "0")}</span>
                  <span className="rank-name"><strong>{chat.displayName}</strong><small>{chat.chatKind === "group" ? "群聊" : "好友"}</small></span>
                  <span className="rank-count">{formatCount(chat.count)}</span>
                  <Icon name="chevron" />
                </button>
              </li>
            ))}
          </ol>
        </section>
      </div>

      <section className="panel relationship-panel">
        <div className="panel-heading"><div><p className="eyebrow">RELATIONSHIPS</p><h2>关系洞察</h2></div><span className="subtle">基于今年聊得最多的好友；间隔 6 小时以上算作一次新对话</span></div>
        {insights.relationships.length ? (
          <table className="relationship-table">
            <thead><tr><th>好友</th><th>今年消息</th><th>变化</th><th>由你开启对话</th><th>你的回复</th><th>对方的回复</th></tr></thead>
            <tbody>
              {insights.relationships.map((item) => (
                <tr key={item.chatId}>
                  <td><button type="button" className="link-button" onClick={() => onOpenDetail(item.chatId)}>{item.displayName}</button></td>
                  <td>{formatCount(item.count)}<small>去年 {formatCount(item.previousCount)}</small></td>
                  <td><span className={`trend-badge ${item.trend}`}>{trendLabels[item.trend]}</span></td>
                  <td>{item.initiatedByMe == null ? "—" : `${Math.round(item.initiatedByMe * 100)}%`}</td>
                  <td>{formatMinutes(item.myReplyMinutes)}</td>
                  <td>{formatMinutes(item.theirReplyMinutes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <p className="list-empty">今年还没有和好友的单聊记录</p>}
        {insights.cooling.length > 0 && (
          <div className="cooling-list">
            <p className="eyebrow">QUIETER THIS YEAR</p>
            <ul>
              {insights.cooling.map((item) => (
                <li key={item.chatId}>
                  <button type="button" className="link-button" onClick={() => onOpenDetail(item.chatId)}>{item.displayName}</button>
                  <span>去年 {formatCount(item.previousCount)} 条 → 今年 {formatCount(item.count)} 条</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </section>
    </div>
  );
}
