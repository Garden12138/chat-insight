import type { AccountSummary, PlatformCapabilities } from "../types";
import { Icon } from "../components/Icons";
import { InlineNotice } from "../components/States";

interface DataManagementProps {
  account: AccountSummary;
  capabilities: PlatformCapabilities;
  busy: boolean;
  onSync: () => void;
  onSwitchAccount: () => void;
  onImport: () => void;
  onExport: () => void;
  onClear: () => void;
}

export function DataManagement({ account, capabilities, busy, onSync, onSwitchAccount, onImport, onExport, onClear }: DataManagementProps) {
  const canReadWechat = capabilities.directWechatRead && capabilities.bridgeBundled;

  return (
    <div className="view-content data-view">
      <section className="data-intro">
        <p className="eyebrow">DATA & PRIVACY</p>
        <h2>你的数据，只在这里。</h2>
        <p>Chat Insight 的数据库位于本机应用数据目录。导出文件包含聊天正文，请像保护聊天记录一样妥善保存。</p>
      </section>

      <section className="panel source-card">
        <div className="source-status"><span className="status-dot" /><div><p className="eyebrow">ACTIVE DATASET</p><h3>{account.displayName}</h3><p>{account.messageCount.toLocaleString("zh-CN")} 条消息 · {account.chatCount} 个聊天</p></div></div>
        <dl><div><dt>来源</dt><dd>{account.sourcePlatform}</dd></div><div><dt>最近同步</dt><dd>{account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString("zh-CN") : "尚未同步"}</dd></div><div><dt>运行模式</dt><dd>{capabilities.directWechatRead ? "本地微信只读" : "JSONL 导入"}</dd></div></dl>
      </section>

      <div className="data-action-grid">
        <article className="panel action-card">
          <div className="action-icon coral"><Icon name="refresh" /></div><div><h3>更新本地记录</h3><p>读取新消息并增量写入，不会修改微信原始数据。</p></div>
          <button className="secondary-button" disabled={busy || !canReadWechat} onClick={onSync}>立即同步</button>
        </article>
        {canReadWechat && (
          <article className="panel action-card">
            <div className="action-icon green"><Icon name="switch" /></div><div><h3>切换微信账号</h3><p>先在微信里登录要切换的账号并点开任意一个聊天，再点这里。连接过的账号会直接切换，新账号需要重新完成一次本机授权；每个账号的记录分开保存。</p></div>
            <button className="secondary-button" disabled={busy} onClick={onSwitchAccount}>切换账号</button>
          </article>
        )}
        <article className="panel action-card">
          <div className="action-icon green"><Icon name="download" /></div><div><h3>导出便携数据</h3><p>生成 chat-insight-jsonl v1，可在另一台电脑导入。</p></div>
          <button className="secondary-button" disabled={busy} onClick={onExport}>导出 JSONL</button>
        </article>
        <article className="panel action-card">
          <div className="action-icon gold"><Icon name="upload" /></div><div><h3>导入分析数据</h3><p>导入会去重；未知版本或损坏文件不会写入数据库。</p></div>
          <button className="secondary-button" disabled={busy} onClick={onImport}>选择文件</button>
        </article>
        <article className="panel action-card danger-card">
          <div className="action-icon red"><Icon name="trash" /></div><div><h3>清空当前数据集</h3><p>只删除 Chat Insight 缓存，不会删除任何微信文件。</p></div>
          <button className="danger-button" disabled={busy} onClick={onClear}>清空缓存</button>
        </article>
      </div>

      {!capabilities.directWechatRead && <InlineNotice tone="info">当前平台采用导入分析模式。你仍可以使用全部图表、筛选和聊天详情功能。</InlineNotice>}
    </div>
  );
}
