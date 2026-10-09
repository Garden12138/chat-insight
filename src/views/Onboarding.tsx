import type { PlatformCapabilities, SyncProgress } from "../types";
import { BrandMark } from "../components/BrandMark";
import { Icon } from "../components/Icons";
import { InlineNotice } from "../components/States";

interface OnboardingProps {
  capabilities: PlatformCapabilities;
  busy: boolean;
  progress?: SyncProgress;
  error?: string;
  onBootstrap: () => void;
  onImport: () => void;
  onShowGuide: () => void;
}

export function Onboarding({ capabilities, busy, progress, error, onBootstrap, onImport, onShowGuide }: OnboardingProps) {
  const canConnect = capabilities.directWechatRead && capabilities.bridgeBundled;

  return (
    <main className="onboarding">
      <div className="onboarding-glow" />
      <section className="welcome-card">
        <BrandMark size={60} />
        <p className="eyebrow">LOCAL-FIRST CHAT ANALYTICS</p>
        <h1>把散落在时间里的聊天，<br /><em>重新看见。</em></h1>
        <p className="welcome-copy">
          Chat Insight 在你的电脑上读取并分析微信聊天记录。没有要注册的账号，默认也不会上传或修改任何消息。
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

        {error && !busy && (
          <InlineNotice tone="error">
            <strong>本机授权未完成</strong>
            <p>{error}</p>
          </InlineNotice>
        )}

        {progress?.running && (
          <div className="setup-progress">
            <div className="progress-track"><span style={{ width: `${progress.total ? (progress.current / progress.total) * 100 : 12}%` }} /></div>
            <span>{progress.message}</span>
          </div>
        )}

        <ol className="welcome-steps">
          <li><span>1</span><div><strong>放入聊天</strong><p>连接本机微信，或导入已经导出的 JSONL。</p></div></li>
          <li><span>2</span><div><strong>等同步完成</strong><p>内容只写入这台电脑，同步过程中可以取消。</p></div></li>
          <li><span>3</span><div><strong>跟着页面看一遍</strong><p>进入后会介绍每个功能，也可以现在先看。</p></div></li>
        </ol>

        <div className="welcome-actions">
          {canConnect && (
            <button className="primary-button" onClick={onBootstrap} disabled={busy}>
              <Icon name="refresh" /> {busy ? "正在连接…" : "连接本机微信"}
            </button>
          )}
          <button className={canConnect ? "secondary-button" : "primary-button"} onClick={onImport} disabled={busy}>
            <Icon name="upload" /> 导入 Chat Insight 数据
          </button>
          <button type="button" className="guide-link" onClick={onShowGuide} disabled={busy}>
            <Icon name="help" /> 先看使用指引
          </button>
        </div>

        <p className="platform-note">当前平台：{capabilities.os} · {capabilities.arch}</p>
      </section>
    </main>
  );
}
