import { toLocalDate } from "./utils";

const ISO = /(\d{4})-(\d{2})-(\d{2})/g;
const CN = /(\d{4})年(\d{1,2})月(\d{1,2})日/g;
const INTENT = /发生了什么|发生了哪些|有哪些事情|有什么事情|当天事件|回顾群聊|回顾私聊|事件回顾|群聊回顾|私聊回顾|群聊事件|私聊事件|都发生了什么/;

export interface DateRangeLike {
  from?: string;
  to?: string;
}

export interface EventReviewRoute {
  date: string;
  chatId: string;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function validYmd(year: number, month: number, day: number): string | null {
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCFullYear() !== year || date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) {
    return null;
  }
  return `${year}-${pad(month)}-${pad(day)}`;
}

function firstExplicitDate(text: string): string | null {
  const matches: Array<{ index: number; date: string }> = [];
  for (const pattern of [ISO, CN]) {
    pattern.lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(text))) {
      const date = validYmd(Number(match[1]), Number(match[2]), Number(match[3]));
      if (date) matches.push({ index: match.index, date });
    }
  }
  matches.sort((left, right) => left.index - right.index);
  return matches[0]?.date ?? null;
}

function relativeDate(text: string, today: string): string | null {
  const base = validYmd(...(today.split("-").map(Number) as [number, number, number]));
  if (!base) return null;
  const shift = (days: number) => {
    const [year, month, day] = base.split("-").map(Number);
    const next = new Date(Date.UTC(year, month - 1, day + days));
    return `${next.getUTCFullYear()}-${pad(next.getUTCMonth() + 1)}-${pad(next.getUTCDate())}`;
  };
  if (text.includes("前天")) return shift(-2);
  if (text.includes("昨天")) return shift(-1);
  if (text.includes("今天")) return shift(0);
  return null;
}

/** Parses a single exact local date. Does not guess yearless or fuzzy ranges. */
export function parseExactDate(text: string, today = toLocalDate(new Date())): string | null {
  return firstExplicitDate(text) ?? relativeDate(text, today);
}

export function hasEventReviewIntent(text: string): boolean {
  return INTENT.test(text);
}

export function dateIntersectsRange(date: string, range: DateRangeLike): boolean {
  if (range.from && date < range.from) return false;
  if (range.to && date > range.to) return false;
  return true;
}

export function formatReviewDate(date: string): string {
  const [year, month, day] = date.split("-");
  if (!year || !month || !day) return date;
  return `${year}年${Number(month)}月${Number(day)}日`;
}

/**
 * Routes a normal AI question to event review when the date is exact and the
 * intent is a day-wide recap. A selected chat, group or private, is kept as the
 * review scope instead of being rewritten to all chats.
 */
export function routeEventReview(
  query: string,
  selectedChat?: { id: string; chatKind: string } | null,
  today = toLocalDate(new Date()),
): EventReviewRoute | null {
  const date = parseExactDate(query, today);
  if (!date || !hasEventReviewIntent(query)) return null;
  if (selectedChat?.id) return { date, chatId: selectedChat.id };
  return { date, chatId: "" };
}
