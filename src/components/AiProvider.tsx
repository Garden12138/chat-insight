import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { AiContext, INDEX_PROGRESS_KEY, setAiTaskResult, type ConsentRequest } from "../ai";
import { api } from "../api";
import { aiServiceHost, defaultAiSettings } from "../aiDefaults";
import type { AiSettingsView } from "../types";
import { CloudConsent } from "./CloudConsent";

interface PendingConsent {
  request: ConsentRequest;
  resolve: (accepted: boolean) => void;
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
    if (settings.provider === "xcli") {
      try {
        const privacy = await api.xcliPrivacy();
        if (privacy?.chatCloud === false && privacy.embeddingCloud === false) return false;
      } catch { /* Unknown gateway routes still require explicit consent. */ }
    } else if (settings.isLocal) return false;
    return new Promise<boolean | null>((resolve) => {
      setPending({ request, resolve: (accepted) => resolve(accepted ? true : null) });
    });
  }, [settings.isLocal, settings.provider]);

  const decide = useCallback((accepted: boolean) => {
    pending?.resolve(accepted);
    setPending(undefined);
  }, [pending]);

  const value = useMemo(() => ({ settings, setSettings, confirmCloud }), [settings, confirmCloud]);

  return (
    <AiContext.Provider value={value}>
      {children}
      {pending && <CloudConsent host={settings.provider === "xcli" ? "xcli 配置的上游模型（可能在云端）" : aiServiceHost(settings)} request={pending.request} onDecide={decide} />}
    </AiContext.Provider>
  );
}
