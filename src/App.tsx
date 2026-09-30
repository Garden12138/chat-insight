import { useCallback, useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { AccountSummary, DateRange, PlatformCapabilities, RangePreset, SyncProgress, ViewName } from "./types";
import { DateRangePicker } from "./components/DateRangePicker";
import { Icon } from "./components/Icons";
import { LoadingState } from "./components/States";
import { Onboarding } from "./views/Onboarding";
import { Overview } from "./views/Overview";
import { ChatDetail } from "./views/ChatDetail";
import { DataManagement } from "./views/DataManagement";
import { errorMessage, rangeForPreset } from "./utils";

const navItems: Array<{ id: ViewName; label: string; icon: "overview" | "detail" | "data" }> = [
  { id: "overview", label: "总览", icon: "overview" },
  { id: "detail", label: "聊天详情", icon: "detail" },
  { id: "data", label: "数据管理", icon: "data" },
];

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

  const range = useMemo(() => preset === "custom" ? customRange : rangeForPreset(preset), [preset, customRange]);
  const activeAccount = accounts.find((account) => account.id === accountId) ?? accounts[0];

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

  if (!capabilities) return <div className="app-loading"><div className="brand-mark large"><span /><span /><span /></div><LoadingState label="正在打开本地数据库…" /></div>;
  const toastView = toast && <div className={`toast ${toast.tone}`} role="status"><Icon name={toast.tone === "success" ? "check" : "warning"} />{toast.text}</div>;

  if (!activeAccount) return <><Onboarding capabilities={capabilities} busy={busy} progress={syncProgress} error={setupError} onBootstrap={bootstrapAndSync} onImport={importData} />{toastView}</>;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><span /><span /><span /></div><div><strong>Chat Insight</strong><small>本地聊天分析</small></div></div>
        <nav>
          {navItems.map((item) => <button key={item.id} className={view === item.id ? "active" : ""} onClick={() => setView(item.id)}><Icon name={item.icon} /><span>{item.label}</span></button>)}
        </nav>
        <div className="sidebar-spacer" />
        <div className="local-badge"><span /><div><strong>仅在本机</strong><small>数据未上传</small></div></div>
        <p className="version">Chat Insight v0.1.0</p>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div><p className="eyebrow">{view === "overview" ? "YOUR CHAT LANDSCAPE" : view === "detail" ? "CONVERSATION FOCUS" : "LOCAL DATA CONTROL"}</p><h1>{view === "overview" ? "聊天总览" : view === "detail" ? "聊天详情" : "数据管理"}</h1></div>
          <div className="topbar-actions">
            {view !== "data" && <DateRangePicker preset={preset} range={customRange} onPresetChange={setPreset} onRangeChange={setCustomRange} />}
            <label className="account-select"><span>数据集</span><select value={activeAccount.id} onChange={(event) => setAccountId(event.target.value)}>{accounts.map((account) => <option value={account.id} key={account.id}>{account.displayName}</option>)}</select></label>
            {capabilities.directWechatRead && capabilities.bridgeBundled && <button className="icon-button" title="同步新消息" aria-label="同步新消息" disabled={busy} onClick={runSync}><Icon name="refresh" /></button>}
          </div>
        </header>

        {syncProgress?.running && <div className="sync-strip"><div><span style={{ width: `${syncProgress.total ? (syncProgress.current / syncProgress.total) * 100 : 10}%` }} /></div><p>{syncProgress.message}</p><button onClick={() => api.cancelSync()}>取消</button></div>}
        {view === "overview" && <Overview accountId={activeAccount.id} range={range} revision={revision} onOpenDetail={openDetail} />}
        {view === "detail" && <ChatDetail accountId={activeAccount.id} range={range} revision={revision} requestedChatId={requestedChatId} />}
        {view === "data" && <DataManagement account={activeAccount} capabilities={capabilities} busy={busy} onSync={runSync} onImport={importData} onExport={exportData} onClear={clearData} />}
      </main>

      {toastView}
    </div>
  );
}
