import { describe, expect, it } from "vitest";
import { dueLabel, isOverdue, sortOpen } from "./commitments";
import type { Commitment } from "./types";

const base: Commitment = {
  id: 1,
  chatId: "alice",
  chatName: "小艾",
  chatKind: "direct",
  direction: "mine",
  ownerName: "我",
  title: "把合同发给小艾",
  quote: "周五前我把合同发你",
  status: "open",
  sourceLocalId: 2,
  sourceEndLocalId: 2,
  sourceAt: "2026-10-12T09:01:00+08:00",
  confidence: 0.9,
  userEdited: false,
  createdAt: "2026-10-12T10:00:00Z",
  updatedAt: "2026-10-12T10:00:00Z",
};

describe("commitment dates", () => {
  it("describes how far away a due date is", () => {
    expect(dueLabel("2026-10-09", "2026-10-12")).toBe("已过期 3 天");
    expect(dueLabel("2026-10-12", "2026-10-12")).toBe("今天到期");
    expect(dueLabel("2026-10-13", "2026-10-12")).toBe("明天到期");
    expect(dueLabel("2026-11-01", "2026-10-30")).toBe("还有 2 天");
  });

  it("flags only open items past their date", () => {
    expect(isOverdue({ ...base, dueDate: "2026-10-11" }, "2026-10-12")).toBe(true);
    expect(isOverdue({ ...base, dueDate: "2026-10-12" }, "2026-10-12")).toBe(false);
    expect(isOverdue({ ...base, status: "done", dueDate: "2026-10-01" }, "2026-10-12")).toBe(false);
    expect(isOverdue(base, "2026-10-12")).toBe(false);
  });

  it("puts the nearest due dates first and undated items last", () => {
    const sorted = sortOpen([
      { ...base, id: 1 },
      { ...base, id: 2, dueDate: "2026-10-20" },
      { ...base, id: 3, dueDate: "2026-10-10" },
      { ...base, id: 4, sourceAt: "2026-10-13T09:00:00+08:00" },
    ]);
    expect(sorted.map((item) => item.id)).toEqual([3, 2, 4, 1]);
  });
});
