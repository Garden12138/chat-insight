import type { ChatFootprintEntry, DateRange } from "./types";
import { rangeForPreset } from "./utils";

export interface FootprintDay {
  localDate: string;
  weekday: string;
  day: string;
  month: string;
  chatCount: number;
  messageCount: number;
  chats: ChatFootprintEntry[];
}

const WEEKDAYS = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];

export function footprintQueryRange(range: DateRange, showAll: boolean, now = new Date()): DateRange {
  if (showAll) return {};
  if (range.from || range.to) return range;
  return rangeForPreset("30d", now);
}

export function groupFootprint(entries: ChatFootprintEntry[]): FootprintDay[] {
  const days: FootprintDay[] = [];
  for (const entry of entries) {
    const current = days.at(-1);
    if (!current || current.localDate !== entry.localDate) days.push(dayFrom(entry.localDate, [entry]));
    else current.chats.push(entry);
  }
  return days.map((day) => ({
    ...day,
    chatCount: day.chats.length,
    messageCount: day.chats.reduce((total, chat) => total + chat.messageCount, 0),
  }));
}

function dayFrom(localDate: string, chats: ChatFootprintEntry[]): FootprintDay {
  const parsed = new Date(`${localDate}T12:00:00`);
  const weekday = Number.isNaN(parsed.getTime()) ? "" : WEEKDAYS[parsed.getDay()];
  const [, month = "", day = ""] = localDate.split("-");
  return {
    localDate,
    weekday,
    day,
    month: month ? `${Number(month)}月` : "",
    chatCount: chats.length,
    messageCount: chats.reduce((total, chat) => total + chat.messageCount, 0),
    chats,
  };
}

export function clock(value: string): string {
  const matched = value.match(/T(\d{2}:\d{2})/);
  return matched?.[1] ?? value;
}
