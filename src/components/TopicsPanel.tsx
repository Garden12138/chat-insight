import { useEffect, useState } from "react";
import { api } from "../api";
import { formatChars, getAiTask, newRequestId, runAiTask, setAiTaskResult, stopAiTask, useAi, useAiTask, usesCloud } from "../ai";
import type { DateRange, KeywordCount, TopicsResult } from "../types";
import { formatDateTime } from "../utils";
import { Icon } from "./Icons";
import { LoadingState } from "./States";

interface TopicsPanelProps {
  accountId: string;
  chatId: string;
  chatName: string;
  range: DateRange;
  revision: number;
  keywords: KeywordCount[];
}

function KeywordCloud({ keywords }: { keywords: KeywordCount[] }) {
  return (
    <div className="keyword-cloud">
      {keywords.map((item, index) => <span key={item.word} style={{ fontSize: `${Math.max(13, 21 - index * .35)}px` }}>{item.word}<small>{item.count}</small></span>)}
      {!keywords.length && <p className="list-empty">当前范围内没有足够的文本关键词</p>}
    </div>
  );
}

function monthLabel(value: string) {
  return value ? formatDateTime(value).replace(/\s.*$/, "") : "";
}

export function TopicsPanel({ accountId, chatId, chatName, range, revision, keywords }: TopicsPanelProps) {
  const { settings, confirmCloud } = useAi();
  const key = `topics:${accountId}:${chatId}:${range.from ?? ""}:${range.to ?? ""}`;
  const { status, running, result, error } = useAiTask<TopicsResult>(key);
  const [showKeywords, setShowKeywords] = useState(false);

  useEffect(() => {
    setShowKeywords(false);
    if (!settings.enabled || getAiTask(key).running) return;
    let active = true;
    api.chatTopics(accountId, chatId, range, newRequestId(), "cached")
      .then((value) => { if (active) setAiTaskResult(key, value ?? undefined); })
      .catch(() => undefined);
    return () => { active = false; };
  }, [key, accountId, chatId, range, revision, settings.enabled, settings.chatModel, settings.embedModel]);

  async function analyze(mode: "generate" | "refresh") {
    let detail = `“${chatName}”里还没建立索引的消息，以及归纳时用到的聊天摘录。`;
    if (usesCloud(settings)) {
      const estimate = await api.aiEstimate(accountId, {}, chatId);
      detail = `“${chatName}”共 ${estimate.messages.toLocaleString("zh-CN")} 条文本（约 ${formatChars(estimate.chars)}），以及归纳时用到的聊天摘录。`;
    }
    const consent = await confirmCloud({ title: "发送聊天记录以归纳话题", detail });
    if (consent === null) return;
    setShowKeywords(false);
    await runAiTask(key, (requestId) => api.chatTopics(accountId, chatId, range, requestId, mode, consent));
  }

  const aiOn = settings.enabled;
  const showTopics = aiOn && !showKeywords && (running || !!result);

  return (
    <section className="panel keyword-panel">
      <div className="panel-heading">
        <div><p className="eyebrow">{aiOn ? "TOPICS" : "LOCAL KEYWORDS"}</p><h2>{aiOn ? "常聊话题" : "常聊关键词"}</h2></div>
        {aiOn ? (
          <div className="panel-actions">
            {result && !running && <button type="button" className="link-button" onClick={() => setShowKeywords((value) => !value)}>{showKeywords ? "查看话题" : "查看关键词"}</button>}
            {running
              ? <button type="button" className="secondary-button compact" onClick={() => stopAiTask(key)}><Icon name="stop" />停止</button>
              : <button type="button" className={result ? "secondary-button compact" : "primary-button compact"} onClick={() => analyze(result ? "refresh" : "generate")}><Icon name="sparkle" />{result ? "重新归纳" : "归纳话题"}</button>}
          </div>
        ) : null}
      </div>
      {showTopics ? (
        running ? <LoadingState label={status || "正在归纳话题…"} /> : result && (
          <>
            <div className="topic-grid">
              {result.topics.map((topic, index) => (
                <article key={`${index}-${topic.name}`} className="topic-card">
                  <div className="topic-head"><strong>{topic.name}</strong><span>{Math.round(topic.share * 100)}%</span></div>
                  <div className="topic-bar"><span style={{ width: `${Math.max(4, topic.share * 100)}%` }} /></div>
                  {topic.description && <p>{topic.description}</p>}
                  <small>{monthLabel(topic.firstAt)}{topic.lastAt && topic.lastAt.slice(0, 10) !== topic.firstAt.slice(0, 10) ? ` — ${monthLabel(topic.lastAt)}` : ""} · {topic.chunkCount} 段对话</small>
                </article>
              ))}
            </div>
            {result.generatedAt && <p className="ai-meta topic-meta">生成于 {formatDateTime(result.generatedAt)}</p>}
          </>
        )
      ) : (
        <>
          {aiOn && !result && !showKeywords && <p className="ai-panel-hint topic-hint">点「归纳话题」，把这段聊天归成几个主题。</p>}
          <KeywordCloud keywords={keywords} />
        </>
      )}
      {error && <p className="ai-error topic-error">{error}</p>}
    </section>
  );
}
