import type { Commitment, CommitmentScanOutcome } from "./types";

const DAY_MS = 24 * 60 * 60 * 1000;

function dayNumber(date: string): number {
  const [year, month, day] = date.split("-").map(Number);
  return Math.round(Date.UTC(year, month - 1, day) / DAY_MS);
}

/** Whole days from `today` to `dueDate` (both `YYYY-MM-DD`); negative once overdue. */
export function daysUntil(dueDate: string, today: string): number {
  return dayNumber(dueDate) - dayNumber(today);
}

export function dueLabel(dueDate: string, today: string): string {
  const days = daysUntil(dueDate, today);
  if (days < 0) return `已过期 ${-days} 天`;
  if (days === 0) return "今天到期";
  if (days === 1) return "明天到期";
  return `还有 ${days} 天`;
}

export function isOverdue(item: Commitment, today: string): boolean {
  return item.status === "open" && !!item.dueDate && daysUntil(item.dueDate, today) < 0;
}

/** Open items: dated ones by due date (overdue first), then undated, newest first. */
export function sortOpen(items: Commitment[]): Commitment[] {
  return [...items].sort((a, b) => {
    if (a.dueDate && b.dueDate) return a.dueDate.localeCompare(b.dueDate) || b.sourceAt.localeCompare(a.sourceAt);
    if (a.dueDate) return -1;
    if (b.dueDate) return 1;
    return b.sourceAt.localeCompare(a.sourceAt);
  });
}

export const followupLabel: Record<string, string> = {
  done: "后续消息显示可能已经完成",
  cancelled: "后续消息显示可能已经取消",
  changed: "后续消息显示时间或内容可能有变",
};

export function scanSummary(outcome: CommitmentScanOutcome): string {
  if (outcome.cancelled) return `已暂停扫描：读了 ${outcome.windows} 段，新增 ${outcome.created} 条建议。下次扫描会从中断处继续。`;
  if (!outcome.windows && !outcome.skipped && !outcome.failed) return "当前范围内没有新的聊天需要扫描。";
  const parts = [`扫描了 ${outcome.windows} 段聊天`];
  if (outcome.skipped) parts.push(`跳过 ${outcome.skipped} 段没有约定迹象的聊天`);
  parts.push(`新增 ${outcome.created} 条建议`);
  if (outcome.updated) parts.push(`发现 ${outcome.updated} 条约定有后续`);
  if (outcome.failed) parts.push(`${outcome.failed} 段模型回复无法识别，下次扫描会重试`);
  return `${parts.join("，")}。`;
}

export function directionLabel(item: Commitment): string {
  return item.direction === "mine" ? "我答应的" : `${item.ownerName || "对方"}答应我的`;
}
