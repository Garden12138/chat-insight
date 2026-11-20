import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { AiContext, INDEX_PROGRESS_KEY, isLocalAiTarget, setAiTaskResult, type ConsentRequest } from "../ai";
import { api } from "../api";
import { aiServiceHost, defaultAiSettings, xcliAgentName } from "../aiDefaults";
import type { AiSettingsView, AiTarget } from "../types";
import { CloudConsent } from "./CloudConsent";

interface PendingConsent {
  request: ConsentRequest;
  settings: AiSettingsView;
  host: string;
  notice?: string;
  resolve: (accepted: boolean) => void;
}

const XCLI_NOTICE = "xcli 和所用 CLI 会在本机保存本次发送内容的副本（默认位于 ~/.xcli/chat-insight/ 下的任务目录），Chat Insight 无法清除。如需清理，可在终端删除对应任务目录。";

function destination(settings: AiSettingsView, target: AiTarget): string {
  if (settings.provider !== "xcli") return aiServiceHost(settings);
  if (target === "text") return `本机 xcli → ${xcliAgentName(settings.chatModel)} 的云端模型`;
  if (settings.vectorProvider === "bailian") return "阿里云百炼（向量服务）";
  try {
    return `${new URL(settings.vectorBaseUrl).host}（向量服务）`;
  } catch {
    return "配置的远程向量服务";
  }
}

export function AiProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<AiSettingsView>(defaultAiSettings);
  const [pending, setPending] = useState<PendingConsent>();

  useEffect(() => {
    api.aiSettings().then(setSettings).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!settings.enabled) return;
    let cleanup: () => void = () => undefined;
    let active = true;
    api.onAiIndexProgress((progress) => setAiTaskResult(INDEX_PROGRESS_KEY, progress))
      .then((unlisten) => { if (active) cleanup = unlisten; else unlisten(); });
    return () => { active = false; cleanup(); };
  }, [settings.enabled]);

  const confirmCloud = useCallback(async (request: ConsentRequest) => {
    const targets = request.targets.filter((target) => !isLocalAiTarget(settings, target));
    if (!targets.length) return false;
    const host = [...new Set(targets.map((target) => destination(settings, target)))].join("、");
    const notice = settings.provider === "xcli" && targets.includes("text") ? XCLI_NOTICE : undefined;
    return new Promise<boolean | null>((resolve) => {
      setPending({ request, settings, host, notice, resolve: (accepted) => resolve(accepted ? true : null) });
    });
  }, [settings]);

  const decide = useCallback((accepted: boolean) => {
    pending?.resolve(accepted && pending.settings === settings);
    setPending(undefined);
  }, [pending, settings]);

  const value = useMemo(() => ({ settings, setSettings, confirmCloud }), [settings, confirmCloud]);

  return (
    <AiContext.Provider value={value}>
      {children}
      {pending && <CloudConsent host={pending.host} request={pending.request} notice={pending.notice} onDecide={decide} />}
    </AiContext.Provider>
  );
}
