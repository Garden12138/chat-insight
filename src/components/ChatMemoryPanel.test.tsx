import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetAiState } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import { AiProvider } from "./AiProvider";
import type { Commitment, Decision } from "../types";
import { ChatMemoryPanel } from "./ChatMemoryPanel";

const mocks = vi.hoisted(() => ({
  api: {
    aiSettings: vi.fn(),
    commitments: vi.fn(),
    decisions: vi.fn(),
    updateDecision: vi.fn(),
    commitmentEstimate: vi.fn(),
    decisionEstimate: vi.fn(),
    scanCommitments: vi.fn(),
    scanDecisions: vi.fn(),
    messageContext: vi.fn(),
    cancelAi: vi.fn(),
    onAiStream: vi.fn(),
    onAiIndexProgress: vi.fn(),
  },
}));
vi.mock("../api", () => ({ api: mocks.api }));

const promise: Commitment = {
  id: 1,
  chatId: "team",
  chatName: "项目群",
  chatKind: "group",
  direction: "mine",
  ownerName: "我",
  title: "周五前定稿",
  quote: "我来出设计稿",
  status: "open",
  sourceLocalId: 2,
  sourceEndLocalId: 2,
  sourceAt: "2026-10-12T10:00:00+08:00",
  confidence: 0.9,
  userEdited: false,
  createdAt: "2026-10-12T10:00:00Z",
  updatedAt: "2026-10-12T10:00:00Z",
};

const decision: Decision = {
  id: 4,
  chatId: "team",
  chatName: "项目群",
  kind: "decision",
  title: "周五前完成设计稿",
  ownerName: "小艾",
  quote: "那就周五前定稿",
  speakerName: "我",
  status: "suggested",
  sourceLocalId: 3,
  sourceAt: "2026-10-12T10:01:00+08:00",
  confidence: 0.8,
  userEdited: false,
  createdAt: "2026-10-12T10:00:00Z",
  updatedAt: "2026-10-12T10:00:00Z",
};

function renderPanel(chatKind = "group") {
  return render(
    <AiProvider>
      <ChatMemoryPanel accountId="a" chatId="team" chatName="项目群" chatKind={chatKind} range={{}} revision={0} onOpenDetail={vi.fn()} onOpenCommitments={vi.fn()} />
    </AiProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  resetAiState();
  mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
  mocks.api.commitments.mockResolvedValue([promise]);
  mocks.api.decisions.mockResolvedValue([decision]);
  mocks.api.onAiStream.mockResolvedValue(() => undefined);
  mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
  mocks.api.updateDecision.mockImplementation(async (_account: string, id: number, patch: { status?: Decision["status"] }) => ({ ...decision, id, status: patch.status ?? decision.status }));
  mocks.api.messageContext.mockResolvedValue([{ localId: 3, occurredAt: decision.sourceAt, senderName: "我", isFromMe: true, kind: "text", text: decision.quote }]);
});

describe("chat memory panel", () => {
  it("hides decisions in a direct chat", async () => {
    renderPanel("direct");
    expect(await screen.findByRole("heading", { name: "周五前定稿" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "群聊决策" })).not.toBeInTheDocument();
    expect(mocks.api.decisions).not.toHaveBeenCalled();
  });

  it("confirms a group decision and can mark it overturned", async () => {
    mocks.api.decisions.mockImplementation(async () => mocks.api.updateDecision.mock.calls.length ? [{ ...decision, status: "active" as const }] : [decision]);
    renderPanel();
    expect(await screen.findByRole("heading", { name: "周五前完成设计稿" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    fireEvent.click(await screen.findByRole("tab", { name: /已决定/ }));
    fireEvent.click(await screen.findByRole("button", { name: "标记推翻" }));
    expect(mocks.api.updateDecision).toHaveBeenLastCalledWith("a", 4, { status: "closed" });
  });

  it("opens the source message", async () => {
    renderPanel();
    const card = (await screen.findByRole("heading", { name: "周五前完成设计稿" })).closest("li") as HTMLElement;
    fireEvent.click(within(card).getByRole("button", { name: "查看原话" }));
    expect(await screen.findByRole("dialog", { name: "项目群 的聊天上下文" })).toBeVisible();
    expect(mocks.api.messageContext).toHaveBeenCalledWith("a", "team", 3, 8);
  });
});
