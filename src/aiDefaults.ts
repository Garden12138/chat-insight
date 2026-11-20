import type { AiProvider, AiSettingsView } from "./types";

export const DEFAULT_BASE_URL = "http://127.0.0.1:11434/v1";
export const BAILIAN_BASE_URL = "https://dashscope.aliyuncs.com/compatible-mode/v1";
export const BAILIAN_CHAT_MODEL = "qwen-plus";
export const BAILIAN_EMBED_MODEL = "text-embedding-v3";

export const defaultAiSettings: AiSettingsView = {
  provider: "openai_compatible",
  enabled: false,
  baseUrl: DEFAULT_BASE_URL,
  chatModel: "qwen3:8b",
  embedModel: "bge-m3",
  vectorProvider: null,
  vectorBaseUrl: DEFAULT_BASE_URL,
  allowCloud: false,
  autoScanCommitments: false,
  hasApiKey: false,
  hasVectorApiKey: false,
  textIsLocal: true,
  vectorsIsLocal: true,
  isLocal: true,
  host: "127.0.0.1",
};

export const xcliDefaults = {
  baseUrl: "xcli",
  chatModel: "auto",
  embedModel: "bge-m3",
  vectorProvider: null,
  vectorBaseUrl: DEFAULT_BASE_URL,
};

export const XCLI_SCAN_UNAVAILABLE = "xcli 每次调用都要启动 CLI，不适合批量扫描。请切换到本机模型或阿里云百炼后再扫描。";

export function normalizeProvider(provider?: AiProvider): AiProvider {
  return provider === "bailian" || provider === "xcli" ? provider : "openai_compatible";
}

export function xcliAgentName(value: string): string {
  switch (value) {
    case "codex": return "Codex";
    case "antigravity": return "Antigravity";
    case "cursor": return "Cursor";
    default: return "自动选择的 CLI";
  }
}

export function aiServiceHost(settings: Pick<AiSettingsView, "provider" | "host">): string {
  if (settings.provider === "xcli") return "xcli（本机 CLI）";
  return settings.provider === "bailian" ? "阿里云百炼" : settings.host;
}
