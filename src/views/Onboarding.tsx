import type { PlatformCapabilities, SyncProgress } from "../types";
import { Icon } from "../components/Icons";
import { InlineNotice } from "../components/States";

interface OnboardingProps {
  capabilities: PlatformCapabilities;
  busy: boolean;
  progress?: SyncProgress;
  onBootstrap: () => void;
  onImport: () => void;
}

export function Onboarding({ capabilities, busy, progress, onBootstrap, onImport }: OnboardingProps) {
  const canConnect = capabilities.directWechatRead && capabilities.bridgeBundled;

  return (
    <main className="onboarding">
      <div className="onboarding-glow" />
      <section className="welcome-card">
        <div className="brand-mark large"><span /><span /><span /></div>
        <p className="eyebrow">LOCAL-FIRST CHAT ANALYTICS</p>
        <h1>把散落在时间里的聊天，<br /><em>重新看见。</em></h1>
        <p className="welcome-copy">
          Chat Insight 在你的电脑上读取并分析微信聊天记录。没有云端账号，没有数据上传，也不会发送或修改任何消息。
        </p>

        <div className="privacy-row">
          <span><Icon name="check" /> 仅保存在本机</span>
          <span><Icon name="check" /> 微信数据只读</span>
          <span><Icon name="check" /> 随时清空缓存</span>
        </div>

        {canConnect ? (
          <InlineNotice tone="warning">
            <strong>首次连接需要一次本机授权。</strong>
            <p>{capabilities.setupHint}</p>
          </InlineNotice>
        ) : (
          <InlineNotice tone={capabilities.setupState === "bridge_missing" ? "error" : "info"}>
            <strong>{capabilities.arch === "x86_64" && capabilities.os === "macos" ? "这台 Intel Mac 使用导入分析模式" : "当前安装不包含微信数据桥"}</strong>
            <p>{capabilities.setupHint}</p>
          </InlineNotice>
        )}

        {progress?.running && (
          <div className="setup-progress">
            <div className="progress-track"><span style={{ width: `${progress.total ? (progress.current / progress.total) * 100 : 12}%` }} /></div>
            <span>{progress.message}</span>
          </div>
        )}

        <div className="welcome-actions">
          {canConnect && (
            <button className="primary-button" onClick={onBootstrap} disabled={busy}>
              <Icon name="refresh" /> {busy ? "正在连接…" : "连接本机微信"}
            </button>
          )}
          <button className={canConnect ? "secondary-button" : "primary-button"} onClick={onImport} disabled={busy}>
            <Icon name="upload" /> 导入 Chat Insight 数据
          </button>
        </div>

        <p className="platform-note">当前平台：{capabilities.os} · {capabilities.arch}</p>
      </section>
    </main>
  );
}
