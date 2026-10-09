import { createContext, useCallback, useContext, useEffect, useState, useSyncExternalStore } from "react";
import { api } from "./api";
import { defaultAiSettings } from "./aiDefaults";
import type { AiSettingsView } from "./types";
import { errorMessage } from "./utils";

export interface ConsentRequest {
  title: string;
  detail: string;
}

export interface AiContextValue {
  settings: AiSettingsView;
  setSettings: (settings: AiSettingsView) => void;
  /**
   * Asks before chat content leaves the machine. Resolves to the `cloudConsent` flag to
   * send, or `null` when the user declined. Local endpoints resolve to `false` at once.
   */
  confirmCloud: (request: ConsentRequest) => Promise<boolean | null>;
}

export const AiContext = createContext<AiContextValue>({
  settings: defaultAiSettings,
  setSettings: () => undefined,
  confirmCloud: async () => false,
});

export const useAi = () => useContext(AiContext);

export const STOPPED_MESSAGE = "已停止生成";

export function usesCloud(settings: AiSettingsView): boolean {
  return settings.enabled && !settings.isLocal && settings.allowCloud;
}

export function newRequestId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function formatChars(chars: number): string {
  return chars >= 10000 ? `${(chars / 10000).toFixed(1)} 万字` : `${chars} 字`;
}

// AI tasks live outside React components so a running request, its streamed text and
// its result survive navigating away from the page that started it.

export interface AiTask<T = unknown> {
  running: boolean;
  /** Text streamed so far for the current or last run. */
  text: string;
  status: string;
  result?: T;
  error: string;
  /** Free-form caption for the run, e.g. the question being answered. */
  label: string;
  requestId?: string;
}

const IDLE: AiTask<never> = Object.freeze({ running: false, text: "", status: "", error: "", label: "" });
const tasks = new Map<string, AiTask>();
const listeners = new Set<() => void>();

function updateTask(key: string, patch: Partial<AiTask>) {
  tasks.set(key, { ...(tasks.get(key) ?? IDLE), ...patch });
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function getAiTask<T>(key: string): AiTask<T> {
  return (tasks.get(key) ?? IDLE) as AiTask<T>;
}

export function useAiTask<T>(key: string): AiTask<T> {
  return useSyncExternalStore(subscribe, () => getAiTask<T>(key));
}

function runningKinds(): string {
  return [...new Set([...tasks].filter(([, task]) => task.running).map(([key]) => key.split(":")[0]))].sort().join(",");
}

/** Comma-separated kinds (the key prefix before `:`) of tasks that are currently running. */
export function useRunningAiKinds(): string {
  return useSyncExternalStore(subscribe, runningKinds);
}

/** Stores a result that did not come from a run (e.g. read from cache); ignored while running. */
export function setAiTaskResult<T>(key: string, result: T | undefined) {
  if (getAiTask(key).running) return;
  updateTask(key, { result, text: "", error: "" });
}

/**
 * Runs `task` under `key` unless one is already running there. Streamed deltas for its
 * request id accumulate in `text`; `reset` clears the previous result first.
 */
export async function runAiTask<T>(
  key: string,
  task: (requestId: string) => Promise<T | null>,
  options: { reset?: boolean; label?: string } = {},
): Promise<void> {
  if (getAiTask(key).running) return;
  const requestId = newRequestId();
  updateTask(key, {
    running: true,
    text: "",
    status: "",
    error: "",
    label: options.label ?? "",
    requestId,
    ...(options.reset ? { result: undefined } : {}),
  });
  const unlisten = await api.onAiStream((event) => {
    const current = tasks.get(key);
    if (event.requestId !== requestId || current?.requestId !== requestId) return;
    updateTask(key, { text: current.text + event.delta, status: event.status || current.status });
  });
  try {
    const result = await task(requestId);
    updateTask(key, { running: false, status: "", result: result ?? undefined });
  } catch (reason) {
    const message = errorMessage(reason);
    updateTask(key, { running: false, status: "", error: message === STOPPED_MESSAGE ? "" : message });
  } finally {
    unlisten();
  }
}

/** Latest `ai-index-progress` event, kept so the settings page can show it after remounting. */
export const INDEX_PROGRESS_KEY = "index-progress";

export function stopAiTask(key: string) {
  const { requestId, running } = getAiTask(key);
  if (running && requestId) api.cancelAi(requestId).catch(() => undefined);
}

const session = new Map<string, unknown>();

/** Like `useState`, but the value is kept for the session across unmounts. */
export function useSessionState<T>(key: string, initial: T) {
  const [state, setState] = useState<{ key: string; value: T }>(() => ({ key, value: session.has(key) ? session.get(key) as T : initial }));
  const value = state.key === key ? state.value : (session.has(key) ? session.get(key) as T : initial);
  useEffect(() => { session.set(key, value); }, [key, value]);
  const setValue = useCallback((next: T) => setState({ key, value: next }), [key]);
  return [value, setValue] as const;
}

/** Test helper: forgets all tasks and session state. */
export function resetAiState() {
  tasks.clear();
  session.clear();
  listeners.forEach((listener) => listener());
}
