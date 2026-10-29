import type { AiProvider, AiSettingsView } from "./types";

export const BAILIAN_BASE_URL = "https://dashscope.aliyuncs.com/compatible-mode/v1";
export const BAILIAN_CHAT_MODEL = "qwen-plus";
export const BAILIAN_EMBED_MODEL = "text-embedding-v3";

export const defaultAiSettings: AiSettingsView = {
  provider: "openai_compatible",
  enabled: false,
  baseUrl: "http://127.0.0.1:11434/v1",
  chatModel: "qwen3:8b",
  embedModel: "bge-m3",
  allowCloud: false,
  autoScanCommitments: false,
  hasApiKey: false,
  isLocal: true,
  host: "127.0.0.1",
};

export function normalizeProvider(provider?: AiProvider): Exclude<AiProvider, "xcli"> {
  return provider === "bailian" ? "bailian" : "openai_compatible";
}

export function aiServiceHost(settings: Pick<AiSettingsView, "provider" | "host">): string {
  return settings.provider === "bailian" ? "阿里云百炼" : settings.host;
}
