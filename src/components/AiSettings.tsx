import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api } from "../api";
import { INDEX_PROGRESS_KEY, setAiTaskResult, useAi, useAiTask } from "../ai";
import type { AiIndexProgress, AiIndexStatus, AiSettings as AiSettingsValues, AiSettingsView } from "../types";
import { errorMessage } from "../utils";
import { Icon } from "./Icons";
import { InlineNotice } from "./States";

type Feedback = { tone: "info" | "warning" | "error"; text: string };

function pick(view: AiSettingsView): AiSettingsValues {
  return { enabled: view.enabled, baseUrl: view.baseUrl, chatModel: view.chatModel, embedModel: view.embedModel, allowCloud: view.allowCloud };
}

function isLocalUrl(value: string): boolean {
  try {
    const { protocol, hostname } = new URL(value);
    return (protocol === "http:" || protocol === "https:") && ["localhost", "127.0.0.1", "[::1]", "::1"].includes(hostname);
  } catch {
    return false;
  }
}

export function AiSettings({ accountId }: { accountId: string }) {
  const { settings, setSettings, confirmCloud } = useAi();
  const [form, setForm] = useState<AiSettingsValues>(() => pick(settings));
  const [apiKey, setApiKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>();
  const [status, setStatus] = useState<AiIndexStatus>();
  const progress = useAiTask<AiIndexProgress>(INDEX_PROGRESS_KEY).result;
  const setProgress = (value?: AiIndexProgress) => setAiTaskResult(INDEX_PROGRESS_KEY, value);

  useEffect(() => { setForm(pick(settings)); }, [settings]);

  const refreshStatus = useCallback(() => {
    if (!settings.enabled) return;
    api.aiIndexStatus(accountId).then(setStatus).catch(() => setStatus(undefined));
  }, [accountId, settings.enabled]);

  useEffect(() => { refreshStatus(); }, [refreshStatus, settings.embedModel]);

  const progressRunning = progress?.running;
  useEffect(() => {
    if (progressRunning === false) refreshStatus();
  }, [progressRunning, refreshStatus]);

  const local = isLocalUrl(form.baseUrl);
  const update = <K extends keyof AiSettingsValues>(key: K, value: AiSettingsValues[K]) => setForm((current) => ({ ...current, [key]: value }));

  async function save(event?: FormEvent): Promise<boolean> {
    event?.preventDefault();
    setSaving(true);
    setFeedback(undefined);
    try {
      setSettings(await api.saveAiSettings({ ...form, apiKey: apiKey.trim() ? apiKey.trim() : undefined }));
      setApiKey("");
      if (event) setFeedback({ tone: "info", text: "AI 设置已保存" });
      return true;
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
      return false;
    } finally {
      setSaving(false);
    }
  }

  async function removeKey() {
    try {
      setSettings(await api.saveAiSettings({ ...pick(settings), apiKey: "" }));
      setFeedback({ tone: "info", text: "已删除保存的 API Key" });
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
    }
  }

  async function test() {
    if (!(await save())) return;
    setSaving(true);
    try {
      const result = await api.testAi();
      const missing = result.chatModelFound ? "" : `；服务上没有找到对话模型 ${form.chatModel}${local ? `，可运行 ollama pull ${form.chatModel}` : ""}`;
      setFeedback({ tone: missing ? "warning" : "info", text: `连接成功，向量模型可用（${result.embeddingDimension} 维）${missing}` });
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  }

  async function buildIndex(rebuild: boolean) {
    const count = rebuild ? status?.totalTextMessages : status?.pendingMessages;
    const consent = await confirmCloud({
      title: "发送聊天记录以建立语义索引",
      detail: `${(count ?? 0).toLocaleString("zh-CN")} 条文本消息会分段发送给向量模型 ${settings.embedModel}，用于计算语义向量。向量和聊天内容只保存在本机。`,
    });
    if (consent === null) return;
    setFeedback(undefined);
    setProgress({ running: true, phase: "indexing", current: 0, total: 0, chunks: 0, message: "正在准备建立索引…" });
    try {
      await api.buildAiIndex(accountId, rebuild, consent);
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
      setProgress(undefined);
    } finally {
      refreshStatus();
    }
  }

  const indexing = progress?.running || status?.running;
  const coverage = status && status.totalTextMessages > 0 ? 1 - status.pendingMessages / status.totalTextMessages : 0;

  return (
    <section className="panel ai-settings">
      <div className="panel-heading">
        <div><p className="eyebrow">AI ASSISTANT</p><h2>AI 设置</h2></div>
        <span className={`ai-mode-badge ${!settings.enabled ? "off" : settings.isLocal ? "local" : "cloud"}`}>
          {!settings.enabled ? "未开启" : settings.isLocal ? "本机模型" : "云端模型"}
        </span>
      </div>
      <form className="ai-settings-form" onSubmit={save}>
        <label className="switch-row">
          <input type="checkbox" checked={form.enabled} onChange={(event) => update("enabled", event.target.checked)} />
          <span><strong>开启 AI 功能</strong><small>用于搜索问答、聊天摘要、常聊话题和年度回顾。默认关闭。</small></span>
        </label>
        <div className="field-grid">
          <label className="field wide"><span>服务地址（OpenAI 兼容接口）</span><input value={form.baseUrl} onChange={(event) => update("baseUrl", event.target.value)} placeholder="http://127.0.0.1:11434/v1" spellCheck={false} /></label>
          <label className="field"><span>对话模型</span><input value={form.chatModel} onChange={(event) => update("chatModel", event.target.value)} placeholder="qwen3:8b" spellCheck={false} /></label>
          <label className="field"><span>向量模型</span><input value={form.embedModel} onChange={(event) => update("embedModel", event.target.value)} placeholder="bge-m3" spellCheck={false} /></label>
          <label className="field wide">
            <span>API Key（可选）{settings.hasApiKey && <em>已保存</em>}</span>
            <div className="key-row">
              <input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={settings.hasApiKey ? "留空则保留已保存的 Key" : "本机 Ollama 不需要"} autoComplete="off" />
              {settings.hasApiKey && <button type="button" className="link-button" onClick={removeKey}>删除</button>}
            </div>
          </label>
        </div>
        {local ? (
          <p className="ai-panel-hint">推荐使用 <strong>Ollama</strong>：安装后运行 <code>ollama pull {form.chatModel || "qwen3:8b"}</code> 和 <code>ollama pull {form.embedModel || "bge-m3"}</code>。所有计算都在这台电脑上完成。</p>
        ) : (
          <InlineNotice tone="warning">
            <strong>这个地址不在本机</strong>
            <p>使用云端模型时，聊天内容会发送到该服务。每次发送聊天内容前都会再次请你确认；年度回顾只发送统计数字。</p>
            <label className="checkbox-label"><input type="checkbox" checked={form.allowCloud} onChange={(event) => update("allowCloud", event.target.checked)} />允许使用云端模型服务</label>
          </InlineNotice>
        )}
        <div className="form-actions">
          <button type="submit" className="primary-button" disabled={saving}>保存设置</button>
          <button type="button" className="secondary-button" disabled={saving || (!local && !form.allowCloud)} onClick={test}>保存并测试连接</button>
        </div>
        {feedback && <InlineNotice tone={feedback.tone}>{feedback.text}</InlineNotice>}
      </form>

      {settings.enabled && (
        <div className="index-section">
          <div className="index-head">
            <div>
              <h3>语义索引</h3>
              <p>把聊天按对话分段并计算语义向量，用于 AI 问答和话题归纳。新同步的消息需要增量更新。</p>
            </div>
            <div className="index-actions">
              {indexing
                ? <button type="button" className="secondary-button" onClick={() => api.cancelAiIndex()}><Icon name="stop" />暂停</button>
                : <>
                    <button type="button" className="primary-button" disabled={!status?.pendingMessages} onClick={() => buildIndex(false)}>{status?.indexedChunks ? "更新索引" : "建立索引"}</button>
                    {!!status?.indexedChunks && <button type="button" className="link-button" onClick={() => buildIndex(true)}>重建</button>}
                  </>}
            </div>
          </div>
          {status && (
            <dl className="index-stats">
              <div><dt>已索引片段</dt><dd>{status.indexedChunks.toLocaleString("zh-CN")}</dd></div>
              <div><dt>待处理消息</dt><dd>{status.pendingMessages.toLocaleString("zh-CN")}</dd></div>
              <div><dt>覆盖率</dt><dd>{Math.round(coverage * 100)}%</dd></div>
              <div><dt>向量模型</dt><dd>{status.model}</dd></div>
            </dl>
          )}
          {progress && (progress.running || progress.phase !== "indexing") && (
            <div className="setup-progress index-progress" role="status">
              {progress.running && <div className="progress-track"><span style={{ width: `${progress.total ? Math.max(4, (progress.current / progress.total) * 100) : 4}%` }} /></div>}
              <span>{progress.message}{progress.running && progress.chunks ? ` · 已生成 ${progress.chunks} 个片段` : ""}</span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
