import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { AiContext, INDEX_PROGRESS_KEY, setAiTaskResult, type ConsentRequest } from "../ai";
import { api } from "../api";
import { defaultAiSettings } from "../aiDefaults";
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

  const confirmCloud = useCallback((request: ConsentRequest) => {
    if (settings.isLocal) return Promise.resolve(false);
    return new Promise<boolean | null>((resolve) => {
      setPending({ request, resolve: (accepted) => resolve(accepted ? true : null) });
    });
  }, [settings.isLocal]);

  const decide = useCallback((accepted: boolean) => {
    pending?.resolve(accepted);
    setPending(undefined);
  }, [pending]);

  const value = useMemo(() => ({ settings, setSettings, confirmCloud }), [settings, confirmCloud]);

  return (
    <AiContext.Provider value={value}>
      {children}
      {pending && <CloudConsent host={settings.host} request={pending.request} onDecide={decide} />}
    </AiContext.Provider>
  );
}
