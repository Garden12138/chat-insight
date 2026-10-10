import type { AiSettingsView } from "./types";

export const defaultAiSettings: AiSettingsView = {
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
