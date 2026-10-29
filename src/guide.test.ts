import { describe, expect, it } from "vitest";
import { placeGuideCard, previewSteps, tourSteps } from "./guide";

describe("first-run guide placement", () => {
  const viewport = { width: 1360, height: 860 };

  it("puts the card to the right of a sidebar control", () => {
    const placed = placeGuideCard({ top: 180, left: 16, width: 196, height: 42 }, viewport);
    expect(placed.left).toBeGreaterThan(16 + 196);
    expect(placed.top).toBe(180);
  });

  it("puts the card to the left of a control on the right edge", () => {
    const placed = placeGuideCard({ top: 28, left: 980, width: 340, height: 40 }, viewport);
    expect(placed.left + 400).toBeLessThanOrEqual(980);
  });

  it("keeps the card inside a short window", () => {
    const placed = placeGuideCard({ top: 760, left: 16, width: 196, height: 80 }, { width: 1100, height: 700 });
    expect(placed.top).toBeGreaterThanOrEqual(16);
    expect(placed.top + 340).toBeLessThanOrEqual(700);
  });
});

describe("first-run guide copy", () => {
  it("teaches the path before data exists, then walks the real pages", () => {
    expect(previewSteps.map((step) => step.id)).toEqual(["what", "import", "sync", "map", "ai"]);
    expect(previewSteps.every((step) => !step.view && !step.target)).toBe(true);
    expect(tourSteps.map((step) => step.view)).toEqual([
      undefined,
      "overview",
      "overview",
      "commitments",
      "detail",
      "ask",
      "report",
      "data",
      "overview",
    ]);
    expect(new Set(tourSteps.map((step) => step.title)).size).toBe(tourSteps.length);
    expect(previewSteps.find((step) => step.id === "ai")?.body).toContain("阿里云百炼");
    expect(tourSteps.find((step) => step.id === "ask")?.body).toContain("事件回顾");
    expect(tourSteps.find((step) => step.id === "data")?.body).toContain("阿里云百炼");
  });
});
