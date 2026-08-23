import type { DateRange, RangePreset } from "./types";

const pad = (value: number) => String(value).padStart(2, "0");

export function toLocalDate(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function rangeForPreset(preset: RangePreset, now = new Date()): DateRange {
  if (preset === "all" || preset === "custom") return {};
  const from = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  from.setDate(from.getDate() - (preset === "30d" ? 29 : 364));
  return { from: toLocalDate(from), to: toLocalDate(now) };
}

export function formatCount(value: number): string {
  return new Intl.NumberFormat("zh-CN", { notation: value >= 10000 ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}

export function formatDateTime(value?: string): string {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

export const messageKindLabel: Record<string, string> = {
  text: "文本",
  image: "图片",
  video: "视频",
  voice: "语音",
  file: "文件",
  link: "链接",
  sticker: "表情",
  location: "位置",
  call: "通话",
  system: "系统",
  unknown: "其他",
};

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "发生了未知错误，请稍后重试";
}
