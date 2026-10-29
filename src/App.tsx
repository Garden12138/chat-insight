import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { AccountSummary, BootstrapResult, CommitmentCounts, DateRange, PlatformCapabilities, RangePreset, SyncProgress, ViewName } from "./types";
import { BrandMark } from "./components/BrandMark";
import { DateRangePicker } from "./components/DateRangePicker";
import { GettingStarted } from "./components/GettingStarted";
import { Icon } from "./components/Icons";
import { LoadingState } from "./components/States";
import { guideCompleted, markGuideCompleted, type GuideMode } from "./guide";
import { Onboarding } from "./views/Onboarding";
import { Overview } from "./views/Overview";
import { ChatDetail } from "./views/ChatDetail";
import { Ask } from "./views/Ask";
import { Commitments } from "./views/Commitments";
import { Report } from "./views/Report";
import { DataManagement } from "./views/DataManagement";
import { errorMessage, rangeForPreset } from "./utils";
import { aiServiceHost } from "./aiDefaults";
import { runAiTask, useAi, useAiTask, useRunningAiKinds, usesCloud } from "./ai";
import type { IconName } from "./components/Icons";
import { version } from "../package.json";

const navItems: Array<{ id: ViewName; label: string; icon: IconName }> = [
  { id: "overview", label: "总览", icon: "overview" },
  { id: "commitments", label: "约定与待办", icon: "todo" },
  { id: "detail", label: "聊天详情", icon: "detail" },
  { id: "ask", label: "搜索与问答", icon: "sparkle" },
  { id: "report", label: "年度报告", icon: "book" },
  { id: "data", label: "数据管理", icon: "data" },
];

const viewTitles: Record<ViewName, { eyebrow: string; title: string }> = {
  overview: { eyebrow: "YOUR CHAT LANDSCAPE", title: "聊天总览" },
  commitments: { eyebrow: "PROMISES TO KEEP", title: "约定与待办" },
  detail: { eyebrow: "CONVERSATION FOCUS", title: "聊天详情" },
  ask: { eyebrow: "SEARCH & ASK", title: "搜索与问答" },
  report: { eyebrow: "YEAR IN CHATS", title: "年度报告" },
  data: { eyebrow: "LOCAL DATA CONTROL", title: "数据管理" },
};

export default function App() {
  const [capabilities, setCapabilities] = useState<PlatformCapabilities>();
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [accountId, setAccountId] = useState("");
  const [view, setView] = useState<ViewName>("overview");
  const [preset, setPreset] = useState<RangePreset>("all");
  const [customRange, setCustomRange] = useState<DateRange>({});
  const [syncProgress, setSyncProgress] = useState<SyncProgress>();
  const [setupError, setSetupError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const [requestedChatId, setRequestedChatId] = useState<string>();
  const [toast, setToast] = useState<{ tone: "success" | "error"; text: string }>();
  const [guideMode, setGuideMode] = useState<GuideMode | null>(null);
  const guideReturnView = useRef<ViewName>("overview");
  const autoGuide = useRef(false);
  const [commitmentCounts, setCommitmentCounts] = useState<CommitmentCounts>();
  const { settings: aiSettings } = useAi();
  const runningAi = useRunningAiKinds().split(",");
  const busyViews: Partial<Record<ViewName, boolean>> = {
    commitments: runningAi.includes("commitments"),
    detail: runningAi.includes("summary") || runningAi.includes("topics") || runningAi.includes("decisions"),
    ask: runningAi.includes("ask"),
    report: runningAi.includes("report"),
  };
  const range = useMemo(() => preset === "custom" ? customRange : rangeForPreset(preset), [preset, customRange]);
  const activeAccount = accounts.find((account) => account.id === accountId) ?? accounts[0];
  const activeAccountId = activeAccount?.id ?? "";
  const commitmentScanRunning = useAiTask(`commitments:${activeAccountId}`).running;

  useEffect(() => {
    if (!activeAccountId) return;
    let active = true;
    api.commitmentCounts(activeAccountId)
      .then((counts) => { if (active) setCommitmentCounts(counts); })
      .catch(() => undefined);
    return () => { active = false; };
  }, [activeAccountId, revision, commitmentScanRunning]);

  const notify = useCallback((tone: "success" | "error", text: string) => {
    setToast({ tone, text });
    window.setTimeout(() => setToast(undefined), 4200);
  }, []);

  const refreshAccounts = useCallback(async (preferId?: string) => {
    const values = await api.accounts();
    setAccounts(values);
    setAccountId((current) => preferId ?? (current || values[0]?.id || ""));
    return values;
  }, []);

  const closeGuide = useCallback(() => {
    setGuideMode((mode) => {
      if (mode === "tour") {
        markGuideCompleted();
        setView(guideReturnView.current);
      }
      return null;
    });
  }, []);

  function showGuide() {
    guideReturnView.current = view;
    setGuideMode("tour");
  }

  useEffect(() => {
    if (!activeAccount || autoGuide.current || guideCompleted() || guideMode) return;
    if (busy || syncProgress?.running) return;
    autoGuide.current = true;
    guideReturnView.current = view;
    setGuideMode("tour");
  }, [activeAccount, busy, syncProgress?.running, guideMode, view]);

  useEffect(() => {
    Promise.all([api.capabilities(), api.accounts()]).then(([platform, values]) => {
      setCapabilities(platform);
      setAccounts(values);
      setAccountId(values[0]?.id ?? "");
    }).catch((reason) => notify("error", errorMessage(reason)));
    let cleanup: () => void = () => {};
    api.onSyncProgress(setSyncProgress).then((unlisten) => { cleanup = unlisten; });
    return () => cleanup();
  }, [notify]);

  async function runSync() {
    setBusy(true);
    try {
      const result = await api.sync();
      await refreshAccounts(result.accountId);
      setRevision((value) => value + 1);
      notify("success", result.cancelled ? "同步已取消，已完成的数据已保存" : `同步完成：新增 ${result.messagesImported} 条消息`);
      if (result.messagesImported > 0 && aiSettings.enabled && aiSettings.isLocal && aiSettings.autoScanCommitments) {
        void runAiTask(`commitments:${result.accountId}`, (requestId) => api.scanCommitments(result.accountId, rangeForPreset("30d"), undefined, requestId, false), { reset: true });
      }
    } catch (reason) {
      setSyncProgress(undefined);
      notify("error", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function bootstrapAndSync() {
    setBusy(true);
    setSetupError(undefined);
    try {
      await api.bootstrap();
      setCapabilities(await api.capabilities());
    } catch (reason) {
      setBusy(false);
      setSyncProgress(undefined);
      setSetupError(errorMessage(reason));
      return;
    }
    setBusy(false);
    await runSync();
  }

  async function switchAccount() {
    setBusy(true);
    let result: BootstrapResult;
    try {
      result = await api.switchAccount();
    } catch (reason) {
      setBusy(false);
      setSyncProgress(undefined);
      notify("error", errorMessage(reason));
      return;
    }
    setBusy(false);
    if (!result.switched) {
      setSyncProgress(undefined);
      const current = accounts.find((account) => account.id === result.accountId)?.displayName;
      notify("error", `微信当前登录的仍是${current ? `“${current}”` : "已连接的账号"}。请先在微信里登录要切换的账号并点开任意一个聊天，再点击切换。`);
      return;
    }
    await runSync();
  }

  async function importData() {
    try {
      const selected = await open({ multiple: false, filters: [{ name: "Chat Insight JSONL", extensions: ["jsonl"] }] });
      if (!selected || Array.isArray(selected)) return;
      setBusy(true);
      const result = await api.importJsonl(selected);
      await refreshAccounts(result.accountId);
      setRevision((value) => value + 1);
      notify("success", `已导入 ${result.messagesImported} 条消息`);
    } catch (reason) {
      notify("error", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function exportData() {
    if (!activeAccount) return;
    try {
      const path = await save({ defaultPath: `${activeAccount.displayName || "chat-insight"}.chat-insight.jsonl`, filters: [{ name: "Chat Insight JSONL", extensions: ["jsonl"] }] });
      if (!path) return;
      setBusy(true);
      const result = await api.exportJsonl(activeAccount.id, path);
      notify("success", `已导出 ${result.messagesExported} 条消息`);
    } catch (reason) {
      notify("error", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function clearData() {
    if (!activeAccount || !window.confirm(`确定清空“${activeAccount.displayName}”的 Chat Insight 缓存吗？\n微信原始数据不会受到影响。`)) return;
    try {
      setBusy(true);
      await api.clearAccount(activeAccount.id);
      await refreshAccounts();
      setRevision((value) => value + 1);
      notify("success", "当前数据集已清空");
    } catch (reason) {
      notify("error", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  function openDetail(chatId: string) {
    setRequestedChatId(chatId);
    setView("detail");
  }

  if (!capabilities) return <div className="app-loading"><BrandMark size={56} /><LoadingState label="正在打开本地数据库…" /></div>;
  const toastView = toast && <div className={`toast ${toast.tone}`} role="status"><Icon name={toast.tone === "success" ? "check" : "warning"} />{toast.text}</div>;
  const guideView = <GettingStarted open={guideMode !== null} mode={guideMode ?? "tour"} view={view} onView={setView} onClose={closeGuide} />;

  if (!activeAccount) return <><Onboarding capabilities={capabilities} busy={busy} progress={syncProgress} error={setupError} onBootstrap={bootstrapAndSync} onImport={importData} onShowGuide={() => setGuideMode("preview")} />{guideView}{toastView}</>;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><BrandMark size={36} /><div><strong>Chat Insight</strong><small>本地聊天分析</small></div></div>
        <nav>
          {navItems.map((item) => (
            <button key={item.id} data-guide={`nav-${item.id}`} className={view === item.id ? "active" : ""} onClick={() => setView(item.id)}>
              <Icon name={item.icon} /><span>{item.label}</span>
              {item.id === "commitments" && !!commitmentCounts?.suggested && <b className="nav-badge" title={`${commitmentCounts.suggested} 条约定待确认`}>{commitmentCounts.suggested > 99 ? "99+" : commitmentCounts.suggested}</b>}
              {busyViews[item.id] && <i className="nav-activity" title="AI 任务进行中" aria-hidden="true" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-spacer" />
        <button type="button" className="guide-button" onClick={showGuide}><Icon name="help" /><span>使用指引</span></button>
        {usesCloud(aiSettings)
          ? <div className="local-badge cloud" data-guide="privacy"><span /><div><strong>已启用云端 AI</strong><small>确认后发送至 {aiServiceHost(aiSettings)}</small></div></div>
          : <div className="local-badge" data-guide="privacy"><span /><div><strong>仅在本机</strong><small>数据未上传</small></div></div>}
        <p className="version">Chat Insight v{version}</p>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div><p className="eyebrow">{viewTitles[view].eyebrow}</p><h1>{viewTitles[view].title}</h1></div>
          <div className="topbar-actions">
            {view !== "data" && view !== "report" && <DateRangePicker preset={preset} range={customRange} onPresetChange={setPreset} onRangeChange={setCustomRange} />}
            <label className="account-select"><span>数据集</span><select value={activeAccount.id} onChange={(event) => setAccountId(event.target.value)}>{accounts.map((account) => <option value={account.id} key={account.id}>{account.displayName}</option>)}</select></label>
            {capabilities.directWechatRead && capabilities.bridgeBundled && <button className="icon-button" title="同步新消息" aria-label="同步新消息" disabled={busy} onClick={runSync}><Icon name="refresh" /></button>}
          </div>
        </header>

        {syncProgress?.running && <div className="sync-strip"><div><span style={{ width: `${syncProgress.total ? (syncProgress.current / syncProgress.total) * 100 : 10}%` }} /></div><p>{syncProgress.message}</p>{syncProgress.phase !== "preparing" && <button onClick={() => api.cancelSync()}>取消</button>}</div>}
        {view === "overview" && <Overview accountId={activeAccount.id} range={range} revision={revision} onOpenDetail={openDetail} />}
        {view === "commitments" && <Commitments accountId={activeAccount.id} range={range} revision={revision} onCountsChange={setCommitmentCounts} onOpenDetail={openDetail} onOpenSettings={() => setView("data")} />}
        {view === "detail" && <ChatDetail accountId={activeAccount.id} range={range} revision={revision} requestedChatId={requestedChatId} onOpenCommitments={() => setView("commitments")} />}
        {view === "ask" && <Ask accountId={activeAccount.id} range={range} revision={revision} onOpenDetail={openDetail} onOpenSettings={() => setView("data")} />}
        {view === "report" && <Report accountId={activeAccount.id} revision={revision} onOpenDetail={openDetail} onOpenSettings={() => setView("data")} />}
        {view === "data" && <DataManagement account={activeAccount} capabilities={capabilities} busy={busy} onSync={runSync} onSwitchAccount={switchAccount} onImport={importData} onExport={exportData} onClear={clearData} />}
      </main>

      {guideView}
      {toastView}
    </div>
  );
}
