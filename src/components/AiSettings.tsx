import { useCallback, useEffect, useState, type FormEvent } from "react";
import { BAILIAN_BASE_URL, BAILIAN_CHAT_MODEL, BAILIAN_EMBED_MODEL, defaultAiSettings, normalizeProvider, xcliAgentName, xcliDefaults } from "../aiDefaults";
import { api } from "../api";
import { hasVectorService, INDEX_PROGRESS_KEY, setAiTaskResult, useAi, useAiTask } from "../ai";
import type { AiIndexProgress, AiIndexStatus, AiProvider, AiSettings as AiSettingsValues, AiSettingsView } from "../types";
import { errorMessage } from "../utils";
import { Icon } from "./Icons";
import { InlineNotice } from "./States";

type Feedback = { tone: "info" | "warning" | "error"; text: string };

function pick(view: AiSettingsView): AiSettingsValues {
  return { provider: normalizeProvider(view.provider), enabled: view.enabled, baseUrl: view.baseUrl, chatModel: view.chatModel, embedModel: view.embedModel, vectorProvider: view.vectorProvider, vectorBaseUrl: view.vectorBaseUrl, allowCloud: view.allowCloud, autoScanCommitments: view.autoScanCommitments };
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
  const [vectorApiKey, setVectorApiKey] = useState("");
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

  useEffect(() => { refreshStatus(); }, [refreshStatus, settings.provider, settings.embedModel, settings.vectorProvider]);

  const progressRunning = progress?.running;
  useEffect(() => {
    if (progressRunning === false) refreshStatus();
  }, [progressRunning, refreshStatus]);

  const bailian = form.provider === "bailian";
  const xcli = form.provider === "xcli";
  const showXcli = !navigator.userAgent.includes("Windows") && /Macintosh|Mac OS X/.test(navigator.userAgent);
  const local = !xcli && !bailian && isLocalUrl(form.baseUrl);
  const vectorBailian = form.vectorProvider === "bailian";
  const vectorCloud = form.vectorProvider !== null && (vectorBailian || !isLocalUrl(form.vectorBaseUrl));
  const testNeedsCloud = xcli ? vectorCloud : !local && !bailian;
  const vectorsAvailable = hasVectorService(settings);
  const update = <K extends keyof AiSettingsValues>(key: K, value: AiSettingsValues[K]) => setForm((current) => ({ ...current, [key]: value }));

  const setProvider = (provider: AiProvider) => {
    setForm((current) => {
      if (provider === current.provider) return current;
      if (provider === "xcli") {
        return { ...current, ...xcliDefaults, provider, allowCloud: false, autoScanCommitments: false };
      }
      if (provider === "bailian") {
        return {
          ...current,
          provider,
          baseUrl: BAILIAN_BASE_URL,
          chatModel: BAILIAN_CHAT_MODEL,
          embedModel: BAILIAN_EMBED_MODEL,
          allowCloud: true,
          autoScanCommitments: false,
        };
      }
      return {
        ...current,
        provider,
        baseUrl: defaultAiSettings.baseUrl,
        chatModel: defaultAiSettings.chatModel,
        embedModel: defaultAiSettings.embedModel,
        allowCloud: false,
      };
    });
    setApiKey("");
    setVectorApiKey("");
  };

  function setVectorProvider(provider: AiSettingsValues["vectorProvider"]) {
    setForm((current) => ({
      ...current,
      vectorProvider: provider,
      vectorBaseUrl: provider === "bailian" ? BAILIAN_BASE_URL : defaultAiSettings.vectorBaseUrl,
      embedModel: provider === "bailian" ? BAILIAN_EMBED_MODEL : defaultAiSettings.embedModel,
    }));
    setVectorApiKey("");
  }

  async function save(event?: FormEvent): Promise<boolean> {
    event?.preventDefault();
    setSaving(true);
    setFeedback(undefined);
    try {
      let vectorKey = xcli ? vectorApiKey.trim() || undefined : undefined;
      if (vectorKey === undefined && xcli && settings.hasVectorApiKey && settings.vectorProvider !== form.vectorProvider) {
        vectorKey = "";
      }
      setSettings(await api.saveAiSettings({ ...form, apiKey: xcli ? undefined : apiKey.trim() || undefined, vectorApiKey: vectorKey }));
      setApiKey("");
      setVectorApiKey("");
      if (event) setFeedback({ tone: "info", text: "AI 设置已保存" });
      return true;
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
      return false;
    } finally {
      setSaving(false);
    }
  }

  async function removeKey(vector = false) {
    try {
      setSettings(await api.saveAiSettings({ ...pick(settings), ...(vector ? { vectorApiKey: "" } : { apiKey: "" }) }));
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
      if (xcli) {
        const agents = result.models.map(xcliAgentName).join("、");
        const vectors = result.embeddingDimension ? `。向量维度 ${result.embeddingDimension}` : "";
        const text = agents ? `可用 CLI：${agents}${vectors}` : "当前隔离配置下没有可用的 CLI";
        setFeedback({ tone: result.chatModelFound ? "info" : "warning", text });
        return;
      }
      const missing = result.chatModelFound ? "" : `。服务上没有找到对话模型 ${form.chatModel}${local ? `，可运行 ollama pull ${form.chatModel}` : ""}`;
      setFeedback({ tone: missing ? "warning" : "info", text: `连接成功${missing}` });
    } catch (reason) {
      setFeedback({ tone: "error", text: errorMessage(reason) });
    } finally {
      setSaving(false);
    }
  }

  async function buildIndex(rebuild: boolean) {
    if (!vectorsAvailable) return;
    const count = rebuild ? status?.totalTextMessages : status?.pendingMessages;
    const consent = await confirmCloud({
      title: "发送聊天记录以建立索引",
      targets: ["vectors"],
      detail: `${(count ?? 0).toLocaleString("zh-CN")} 条文本消息会发送给 ${settings.embedModel}。`,
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
          {!settings.enabled ? "未开启" : settings.provider === "bailian" ? "阿里云百炼" : settings.provider === "xcli" ? "xcli（本机 CLI）" : settings.isLocal ? "本机模型" : "云端模型"}
        </span>
      </div>
      <form className="ai-settings-form" onSubmit={save}>
        <label className="switch-row">
          <input type="checkbox" checked={form.enabled} onChange={(event) => update("enabled", event.target.checked)} />
          <span><strong>开启 AI 功能</strong><small>用于问答、事件回顾、约定、摘要、话题和年度回顾。默认关闭。</small></span>
        </label>
        <div className="field-grid">
          <div className="field wide">
            <span id="ai-provider-label">模型供应商</span>
            <div className="provider-cards" role="radiogroup" aria-labelledby="ai-provider-label">
              <label className={`provider-card ${!bailian && !xcli ? "active" : ""}`}>
                <input type="radio" name="ai-provider" checked={!bailian && !xcli} onChange={() => setProvider("openai_compatible")} aria-label="本机 Ollama / OpenAI 兼容" />
                <strong>本机 Ollama</strong>
                <small>计算留在这台电脑</small>
              </label>
              <label className={`provider-card ${bailian ? "active" : ""}`}>
                <input type="radio" name="ai-provider" checked={bailian} onChange={() => setProvider("bailian")} aria-label="阿里云百炼" />
                <strong>阿里云百炼</strong>
                <small>云端推理，发送聊天前会再次确认</small>
              </label>
              {showXcli && (
                <label className={`provider-card ${xcli ? "active" : ""}`}>
                  <input type="radio" name="ai-provider" checked={xcli} onChange={() => setProvider("xcli")} aria-label="xcli（本机 CLI）" />
                  <strong>xcli（本机 CLI）</strong>
                  <small>使用已登录的 CLI 生成文本</small>
                </label>
              )}
            </div>
          </div>
          {xcli ? (
            <>
              <label className="field wide">
                <span>xcli 可执行文件</span>
                <input value={form.baseUrl} onChange={(event) => update("baseUrl", event.target.value)} placeholder="xcli（默认依次从 XCLI_PATH、PATH、~/.local/bin 查找），或填写绝对路径" spellCheck={false} />
              </label>
              <label className="field">
                <span>生成用的 CLI</span>
                <select value={form.chatModel} onChange={(event) => update("chatModel", event.target.value)}>
                  <option value="auto">自动（推荐）</option>
                  <option value="codex">Codex</option>
                  <option value="antigravity" disabled>Antigravity</option>
                  <option value="cursor" disabled>Cursor</option>
                </select>
              </label>
              <label className="field">
                <span>向量服务</span>
                <select value={form.vectorProvider ?? ""} onChange={(event) => setVectorProvider((event.target.value || null) as AiSettingsValues["vectorProvider"])}>
                  <option value="">不使用</option>
                  <option value="openai_compatible">本机 Ollama / OpenAI 兼容</option>
                  <option value="bailian">阿里云百炼</option>
                </select>
              </label>
              {form.vectorProvider && (
                <>
                  <label className="field"><span>向量服务地址</span><input value={form.vectorBaseUrl} onChange={(event) => update("vectorBaseUrl", event.target.value)} spellCheck={false} /></label>
                  <label className="field"><span>向量模型</span><input value={form.embedModel} onChange={(event) => update("embedModel", event.target.value)} spellCheck={false} /></label>
                  {vectorCloud && (
                    <label className="field wide">
                      <span>向量 API Key</span>
                      <div className="key-row">
                        <input type="password" value={vectorApiKey} onChange={(event) => setVectorApiKey(event.target.value)} placeholder={settings.hasVectorApiKey ? "留空则保留已保存的 Key" : "填写服务凭据"} autoComplete="off" />
                        {settings.hasVectorApiKey && <button type="button" className="link-button" onClick={() => removeKey(true)}>删除</button>}
                      </div>
                    </label>
                  )}
                </>
              )}
            </>
          ) : (
            <>
              <label className="field wide">
                <span>服务地址</span>
                <input value={form.baseUrl} onChange={(event) => update("baseUrl", event.target.value)} placeholder={bailian ? "https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1" : "http://127.0.0.1:11434/v1"} spellCheck={false} />
              </label>
              <label className="field"><span>对话模型</span><input value={form.chatModel} onChange={(event) => update("chatModel", event.target.value)} placeholder={bailian ? BAILIAN_CHAT_MODEL : "qwen3:8b"} spellCheck={false} /></label>
              <label className="field"><span>向量模型</span><input value={form.embedModel} onChange={(event) => update("embedModel", event.target.value)} placeholder={bailian ? BAILIAN_EMBED_MODEL : "bge-m3"} spellCheck={false} /></label>
              <label className="field wide">
                <span>{bailian ? "阿里云百炼 API Key" : "API Key（可选）"}{settings.hasApiKey && <em>已保存</em>}</span>
                <div className="key-row">
                  <input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={settings.hasApiKey ? "留空则保留已保存的 Key" : bailian ? "必填" : "本机 Ollama 不需要"} autoComplete="off" />
                  {settings.hasApiKey && <button type="button" className="link-button" onClick={() => removeKey()}>删除</button>}
                </div>
              </label>
            </>
          )}
        </div>
        {xcli ? (
          <InlineNotice tone={form.allowCloud ? "info" : "warning"}>
            {!form.allowCloud && <strong>xcli 调用的是云端模型，请先允许云端服务</strong>}
            <p>通过本机 Codex CLI 调用云端模型，使用 CLI 已有的登录和额度。每次生成约 10～60 秒，不逐字显示。发送聊天内容前仍会确认。目前支持 macOS 上使用文件凭据的 Codex。</p>
            <label className="checkbox-label"><input type="checkbox" checked={form.allowCloud} onChange={(event) => update("allowCloud", event.target.checked)} />允许使用云端模型服务</label>
          </InlineNotice>
        ) : local ? (
          <>
            <p className="ai-panel-hint">本机推荐 <strong>Ollama</strong>，运行 <code>ollama pull {form.chatModel || "qwen3:8b"}</code> 和 <code>ollama pull {form.embedModel || "bge-m3"}</code>。</p>
            <label className="switch-row">
              <input type="checkbox" checked={form.autoScanCommitments} onChange={(event) => update("autoScanCommitments", event.target.checked)} />
              <span><strong>同步后自动找出新约定</strong><small>每次同步后扫描近 30 天还没看过的聊天，结果放进「约定与待办 → 待确认」。</small></span>
            </label>
          </>
        ) : (
          <InlineNotice tone="warning">
            <strong>{bailian ? "聊天内容会发送到阿里云百炼" : "这个地址不在本机"}</strong>
            <p>{bailian
              ? "发送聊天前会再确认一次。API Key 要和这个地址同一地域。"
              : "发送聊天前会再确认一次。"}</p>
            {!bailian && <label className="checkbox-label"><input type="checkbox" checked={form.allowCloud} onChange={(event) => update("allowCloud", event.target.checked)} />允许使用云端模型服务</label>}
          </InlineNotice>
        )}
        <div className="form-actions">
          <button type="submit" className="primary-button" disabled={saving}>保存设置</button>
          <button type="button" className="secondary-button" disabled={saving || (testNeedsCloud && !form.allowCloud)} onClick={test}>保存并测试连接</button>
        </div>
        {feedback && <InlineNotice tone={feedback.tone}>{feedback.text}</InlineNotice>}
      </form>

      {settings.enabled && (
        <div className="index-section">
          <div className="index-head">
            <div>
              <h3>索引</h3>
              <p>{vectorsAvailable ? "索引用于语义检索和话题归纳。同步新消息后点更新。" : "问答使用关键词检索。配置向量服务后可建立索引。"}</p>
            </div>
            <div className="index-actions">
              {indexing
                ? <button type="button" className="secondary-button" onClick={() => api.cancelAiIndex()}><Icon name="stop" />暂停</button>
                : <>
                    <button type="button" className="primary-button" disabled={!vectorsAvailable || !status?.pendingMessages} onClick={() => buildIndex(false)}>{status?.indexedChunks ? "更新索引" : "建立索引"}</button>
                    {vectorsAvailable && !!status?.indexedChunks && <button type="button" className="link-button" onClick={() => buildIndex(true)}>重建</button>}
                  </>}
            </div>
          </div>
          {status && vectorsAvailable && (
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
              <span>{progress.message}</span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
