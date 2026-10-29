import { describe, expect, it } from "vitest";
import {
  dateIntersectsRange,
  formatReviewDate,
  hasEventReviewIntent,
  parseExactDate,
  routeEventReview,
} from "./eventReviewDate";

const TODAY = "2026-10-29";

describe("parseExactDate", () => {
  it("accepts ISO and Chinese calendar dates", () => {
    expect(parseExactDate("请看 2026-10-27 的记录", TODAY)).toBe("2026-10-27");
    expect(parseExactDate("2026年10月27日发生了什么", TODAY)).toBe("2026-10-27");
    expect(parseExactDate("2026年1月5日", TODAY)).toBe("2026-01-05");
  });

  it("resolves today, yesterday and the day before", () => {
    expect(parseExactDate("今天发生了什么", TODAY)).toBe("2026-10-29");
    expect(parseExactDate("昨天的群聊", TODAY)).toBe("2026-10-28");
    expect(parseExactDate("前天有哪些事情", TODAY)).toBe("2026-10-27");
  });

  it("prefers an explicit date over a relative word", () => {
    expect(parseExactDate("今天先看 2026-10-01 的记录", TODAY)).toBe("2026-10-01");
  });

  it("rejects yearless, fuzzy and impossible dates", () => {
    expect(parseExactDate("10月27日发生了什么", TODAY)).toBeNull();
    expect(parseExactDate("最近发生了什么", TODAY)).toBeNull();
    expect(parseExactDate("上周的群聊", TODAY)).toBeNull();
    expect(parseExactDate("2026-13-01", TODAY)).toBeNull();
    expect(parseExactDate("2025年2月29日", TODAY)).toBeNull();
    expect(parseExactDate("2026年2月30日", TODAY)).toBeNull();
  });

  it("accepts leap-year February 29", () => {
    expect(parseExactDate("2024年2月29日", TODAY)).toBe("2024-02-29");
  });
});

describe("event review intent and routing", () => {
  it("detects recap intent and leaves factual questions alone", () => {
    expect(hasEventReviewIntent("2026年10月27日发生了什么事情")).toBe(true);
    expect(hasEventReviewIntent("当天事件回顾一下")).toBe(true);
    expect(hasEventReviewIntent("回顾群聊里的安排")).toBe(true);
    expect(hasEventReviewIntent("2026年10月27日谁处理了接口问题")).toBe(false);
  });

  it("routes all-chat, group-chat and private-chat recaps", () => {
    expect(routeEventReview("2026-10-27发生了什么", null, TODAY)).toEqual({
      date: "2026-10-27",
      chatId: "",
    });
    expect(routeEventReview("昨天有哪些事情", { id: "g1", chatKind: "group" }, TODAY)).toEqual({
      date: "2026-10-28",
      chatId: "g1",
    });
    expect(routeEventReview("2026-10-27发生了什么", { id: "alice", chatKind: "direct" }, TODAY)).toEqual({
      date: "2026-10-27",
      chatId: "alice",
    });
    expect(routeEventReview("10月27日发生了什么", null, TODAY)).toBeNull();
    expect(routeEventReview("2026-10-27谁处理了接口问题", null, TODAY)).toBeNull();
  });

  it("detects an empty intersection with the top date range", () => {
    expect(dateIntersectsRange("2026-10-27", { from: "2026-01-01", to: "2026-10-31" })).toBe(true);
    expect(dateIntersectsRange("2026-10-27", { from: "2026-01-01", to: "2026-01-31" })).toBe(false);
    expect(dateIntersectsRange("2026-10-27", {})).toBe(true);
    expect(formatReviewDate("2026-10-27")).toBe("2026年10月27日");
  });
});
