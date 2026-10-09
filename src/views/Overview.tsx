import { useEffect, useMemo, useState } from "react";
import type { EChartsOption } from "echarts";
import { api } from "../api";
import type { DashboardData, DateRange } from "../types";
import { Chart } from "../components/Chart";
import { EmptyState, LoadingState } from "../components/States";
import { Icon } from "../components/Icons";
import { chartTheme } from "../theme";
import { formatCount, messageKindLabel } from "../utils";

interface OverviewProps {
  accountId: string;
  range: DateRange;
  revision: number;
  onOpenDetail: (chatId: string) => void;
}

const { axis: chartText, grid: gridLine } = chartTheme;

export function Overview({ accountId, range, revision, onOpenDetail }: OverviewProps) {
  const [data, setData] = useState<DashboardData>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setLoading(true);
    api.dashboard(accountId, range)
      .then((value) => active && setData(value))
      .catch((reason) => active && setError(String(reason)))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [accountId, range, revision]);

  const trendOption = useMemo<EChartsOption>(() => {
    const trend = data?.dailyTrend ?? [];
    return {
      animationDuration: 500,
      tooltip: { ...chartTheme.tooltip, trigger: "axis", valueFormatter: (value) => `${value} 条` },
      grid: { top: 16, right: 18, bottom: 14, left: 12, containLabel: true },
      xAxis: { type: "category", data: trend.map((item) => item.bucket), axisLabel: { color: chartText, hideOverlap: true }, axisLine: { lineStyle: { color: gridLine } } },
      yAxis: { type: "value", minInterval: 1, axisLabel: { color: chartText }, splitLine: { lineStyle: { color: gridLine } } },
      series: [{ type: "line", data: trend.map((item) => item.count), showSymbol: false, smooth: 0.22, lineStyle: { width: 3, color: chartTheme.accent }, areaStyle: { color: chartTheme.areaGradient } }],
    };
  }, [data]);

  const heatmapOption = useMemo<EChartsOption>(() => {
    const weekdays = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    const points = (data?.heatmap ?? []).map((item) => [item.hour, item.weekday, item.count]);
    const max = Math.max(1, ...points.map((item) => Number(item[2])));
    return {
      tooltip: { ...chartTheme.tooltip, formatter: (params: unknown) => {
        const value = (params as { value: number[] }).value;
        return `${weekdays[value[1]]} ${String(value[0]).padStart(2, "0")}:00<br/>${value[2]} 条消息`;
      } },
      grid: { top: 12, right: 24, bottom: 42, left: 46 },
      xAxis: { type: "category", data: Array.from({ length: 24 }, (_, i) => String(i)), axisLabel: { color: chartText, interval: 2 }, splitArea: { show: false } },
      yAxis: { type: "category", data: weekdays, axisLabel: { color: chartText } },
      visualMap: { min: 0, max, show: false, inRange: { color: chartTheme.heatmap } },
      series: [{ type: "heatmap", data: points, itemStyle: { borderRadius: 5, borderColor: "transparent", borderWidth: 3 }, emphasis: { itemStyle: { shadowBlur: 10, shadowColor: "rgba(156,42,28,.25)" } } }],
    };
  }, [data]);

  const typeOption = useMemo<EChartsOption>(() => ({
    tooltip: { ...chartTheme.tooltip, trigger: "item", formatter: "{b}<br/>{c} 条 · {d}%" },
    legend: { bottom: 0, icon: "circle", itemWidth: 8, itemHeight: 8, textStyle: { color: chartText } },
    series: [{ type: "pie", radius: ["52%", "76%"], center: ["50%", "43%"], padAngle: 3, itemStyle: { borderRadius: 8 }, label: { show: false }, data: (data?.messageTypes ?? []).map((item) => ({ name: messageKindLabel[item.kind] ?? item.kind, value: item.count })), color: chartTheme.palette }],
  }), [data]);

  if (loading) return <LoadingState />;
  if (error) return <EmptyState title="暂时无法读取分析结果" description={error} />;
  if (!data || data.metrics.totalMessages === 0) return <EmptyState title="还没有可分析的消息" description="完成一次微信同步，或从其他设备导入 Chat Insight 数据。" />;

  const cards = [
    { label: "消息总数", value: formatCount(data.metrics.totalMessages), icon: "message" as const, meta: "已同步记录" },
    { label: "活跃天数", value: formatCount(data.metrics.activeDays), icon: "calendar" as const, meta: "有聊天的日子" },
    { label: "聊天数量", value: formatCount(data.metrics.chatCount), icon: "users" as const, meta: "好友与群聊" },
    { label: "我的发送占比", value: `${Math.round(data.metrics.sentRatio * 100)}%`, icon: "send" as const, meta: "在全部消息中" },
  ];

  return (
    <div className="view-content overview-view">
      <div className="metric-grid">
        {cards.map((card) => (
          <article className="metric-card" key={card.label}>
            <div className="metric-icon"><Icon name={card.icon} /></div>
            <div><span>{card.label}</span><strong>{card.value}</strong><small>{card.meta}</small></div>
          </article>
        ))}
      </div>

      <div className="dashboard-grid">
        <section className="panel span-2 chart-panel">
          <div className="panel-heading"><div><p className="eyebrow">MESSAGE RHYTHM</p><h2>消息趋势</h2></div></div>
          <Chart option={trendOption} fill ariaLabel="消息数量趋势图" />
        </section>
        <section className="panel">
          <div className="panel-heading"><div><p className="eyebrow">CONVERSATIONS</p><h2>最常聊天</h2></div></div>
          <ol className="ranking-list">
            {data.topChats.map((chat, index) => (
              <li key={chat.chatId}>
                <button onClick={() => onOpenDetail(chat.chatId)}>
                  <span className="rank">{String(index + 1).padStart(2, "0")}</span>
                  <span className="rank-name"><strong>{chat.displayName}</strong><small>{chat.chatKind === "group" ? "群聊" : "好友"}</small></span>
                  <span className="rank-count">{formatCount(chat.count)}</span>
                  <Icon name="chevron" />
                </button>
              </li>
            ))}
          </ol>
        </section>
        <section className="panel span-2">
          <div className="panel-heading"><div><p className="eyebrow">ACTIVE HOURS</p><h2>一周活跃时段</h2></div></div>
          <Chart option={heatmapOption} height={300} ariaLabel="星期和小时活跃热力图" />
        </section>
        <section className="panel">
          <div className="panel-heading"><div><p className="eyebrow">MESSAGE TYPES</p><h2>消息类型</h2></div></div>
          <Chart option={typeOption} height={300} ariaLabel="消息类型分布图" />
        </section>
      </div>
    </div>
  );
}
