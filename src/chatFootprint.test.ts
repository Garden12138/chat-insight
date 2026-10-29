import { describe, expect, it } from "vitest";
import { footprintQueryRange, groupFootprint } from "./chatFootprint";
import type { ChatFootprintEntry } from "./types";

const entry = (localDate: string, chatId: string, messageCount: number, chatKind = "direct"): ChatFootprintEntry => ({
  localDate,
  chatId,
  displayName: chatId,
  chatKind,
  messageCount,
  firstMessageAt: `${localDate}T08:00:00+08:00`,
  lastMessageAt: `${localDate}T18:00:00+08:00`,
});

describe("chat footprint grouping", () => {
  it("keeps newest days first and sums every chat on that day", () => {
    const days = groupFootprint([
      entry("2026-10-28", "amy", 2),
      entry("2026-10-28", "team", 1, "group"),
      entry("2026-10-27", "alice", 4),
    ]);
    expect(days.map((day) => day.localDate)).toEqual(["2026-10-28", "2026-10-27"]);
    expect(days[0].chatCount).toBe(2);
    expect(days[0].messageCount).toBe(3);
    expect(days[0].chats.map((chat) => chat.chatId)).toEqual(["amy", "team"]);
    expect(days[0].weekday).toBe("星期三");
  });

  it("uses the last 30 days when the global range is open", () => {
    expect(footprintQueryRange({}, false, new Date("2026-10-29T12:00:00"))).toEqual({ from: "2026-09-30", to: "2026-10-29" });
    expect(footprintQueryRange({ from: "2026-10-01", to: "2026-10-02" }, false)).toEqual({ from: "2026-10-01", to: "2026-10-02" });
    expect(footprintQueryRange({ from: "2026-10-01" }, true)).toEqual({});
  });
});
