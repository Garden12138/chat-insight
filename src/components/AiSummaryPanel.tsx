import { useEffect } from "react";
import { api } from "../api";
import { formatChars, getAiTask, newRequestId, runAiTask, setAiTaskResult, stopAiTask, useAi, useAiTask, usesCloud } from "../ai";
import type { DateRange, SummaryResult } from "../types";
import { formatDateTime } from "../utils";
import { Icon } from "./Icons";
import { Markdown } from "./Markdown";
import { LoadingState } from "./States";

interface AiSummaryPanelProps {
  accountId: string;
  chatId: string;
  chatName: string;
  range: DateRange;
  revision: number;
  suspendAutoAi?: boolean;
}

export function AiSummaryPanel({ accountId, chatId, chatName, range, revision, suspendAutoAi = false }: AiSummaryPanelProps) {
  const { settings, confirmCloud } = useAi();
  const key = `summary:${accountId}:${chatId}:${range.from ?? ""}:${range.to ?? ""}`;
  const task = useAiTask<SummaryResult>(key);

  useEffect(() => {
    if (suspendAutoAi || !settings.enabled || getAiTask(key).running) return;
    let active = true;
    api.summarizeChat(accountId, chatId, range, newRequestId(), "cached")
      .then((value) => { if (active) setAiTaskResult(key, value ?? undefined); })
      .catch(() => undefined);
    return () => { active = false; };
  }, [key, accountId, chatId, range, revision, settings.enabled, settings.chatModel, suspendAutoAi]);

  async function generate(mode: "generate" | "refresh") {
    let detail = `“${chatName}”在当前日期范围内的文本消息。`;
    if (usesCloud(settings)) {
      const estimate = await api.aiEstimate(accountId, range, chatId);
      detail = `“${chatName}”在当前日期范围内的 ${estimate.messages.toLocaleString("zh-CN")} 条文本消息（约 ${formatChars(estimate.chars)}）。`;
    }
    const consent = await confirmCloud({ title: "发送聊天记录以生成摘要", detail });
    if (consent === null) return;
    await runAiTask(key, (requestId) => api.summarizeChat(accountId, chatId, range, requestId, mode, consent));
  }

  const { running, text, status, result, error } = task;
  let body;
  if (!settings.enabled) {
    body = <p className="ai-panel-hint">到「数据管理 → AI 设置」里开启后，可以总结这段时间聊了什么。</p>;
  } else if (running) {
    body = text ? <><Markdown text={text} /><p className="stream-status"><span className="spinner" />{status || "正在生成…"}</p></> : <LoadingState label={status || "正在准备…"} />;
  } else if (result) {
    body = (
      <>
        <Markdown text={result.text} />
        <p className="ai-meta">基于 {result.messageCount.toLocaleString("zh-CN")} 条文本消息{result.generatedAt ? ` · 生成于 ${formatDateTime(result.generatedAt)}` : ""}</p>
      </>
    );
  } else if (text) {
    body = <><Markdown text={text} /><p className="ai-meta">已停止生成</p></>;
  } else {
    body = <p className="ai-panel-hint">整理这段时间聊了什么。</p>;
  }

  return (
    <section className="panel ai-panel" aria-live="polite">
      <div className="panel-heading">
        <div><p className="eyebrow">AI SUMMARY</p><h2>聊天摘要</h2></div>
        {settings.enabled && (running
          ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(key)}><Icon name="stop" />停止</button>
          : <button type="button" className={result ? "secondary-button compact" : "primary-button compact"} onClick={() => generate(result ? "refresh" : "generate")}><Icon name="sparkle" />{result ? "重新生成" : "生成摘要"}</button>)}
      </div>
      <div className="ai-panel-body">
        {body}
        {error && <p className="ai-error">{error}</p>}
      </div>
    </section>
  );
}
