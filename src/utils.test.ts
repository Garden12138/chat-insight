import { describe, expect, it } from "vitest";
import { rangeForPreset, toLocalDate } from "./utils";

describe("date range helpers", () => {
  const now = new Date(2026, 7, 23, 12, 0, 0);

  it("builds inclusive 30 day ranges", () => {
    expect(rangeForPreset("30d", now)).toEqual({ from: "2026-07-25", to: "2026-08-23" });
  });

  it("keeps all-time range unbounded", () => {
    expect(rangeForPreset("all", now)).toEqual({});
  });

  it("formats local dates without UTC shifting", () => {
    expect(toLocalDate(new Date(2026, 0, 2, 1))).toBe("2026-01-02");
  });
});
