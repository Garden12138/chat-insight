import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { resetAiState } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import { AiProvider } from "../components/AiProvider";
import type { ReportResult, YearInsights } from "../types";
import { Report } from "./Report";

const mocks = vi.hoisted(() => ({
  api: {
    aiSettings: vi.fn(),
    cancelAi: vi.fn(),
    onAiIndexProgress: vi.fn(),
    onAiStream: vi.fn(),
    yearInsights: vi.fn(),
    yearReport: vi.fn(),
  },
}));

vi.mock("../api", () => ({ api: mocks.api }));
vi.mock("../components/Chart", () => ({ Chart: () => null }));

it("stops an xcli annual report using the active request ID", async () => {
  resetAiState();
  mocks.api.aiSettings.mockResolvedValue({
    ...defaultAiSettings, enabled: true, provider: "xcli", chatModel: "codex",
    allowCloud: true, isLocal: false, textIsLocal: false,
  });
  mocks.api.yearInsights.mockResolvedValue({
    year: "2026", availableYears: ["2026"], totalMessages: 1, sentMessages: 1,
    activeDays: 1, chatCount: 1, previousTotal: 0, monthly: [], topChats: [],
    messageTypes: [], relationships: [], cooling: [],
  } satisfies YearInsights);
  mocks.api.cancelAi.mockResolvedValue(undefined);
  mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
  mocks.api.onAiStream.mockResolvedValue(() => undefined);
  let cancel: (error: Error) => void = () => undefined;
  mocks.api.yearReport.mockImplementation((_account, _year, _requestId, mode) => {
    if (mode === "cached") return Promise.resolve(null);
    return new Promise<ReportResult>((_, reject) => { cancel = reject; });
  });
  render(<AiProvider><Report accountId="synthetic" revision={0} onOpenDetail={vi.fn()} onOpenSettings={vi.fn()} /></AiProvider>);

  fireEvent.click(await screen.findByRole("button", { name: "生成回顾" }));
  const stop = await screen.findByRole("button", { name: "停止" });
  const generation = mocks.api.yearReport.mock.calls.find((args) => args[3] === "generate");
  expect(generation).toEqual(["synthetic", "2026", expect.any(String), "generate", true]);
  fireEvent.click(stop);
  expect(mocks.api.cancelAi).toHaveBeenCalledWith(generation?.[2]);
  await act(async () => cancel(new Error("已停止生成")));
  expect(screen.getByRole("button", { name: "生成回顾" })).toBeEnabled();
});
