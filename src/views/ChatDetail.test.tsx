import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetAiState } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import { AiProvider } from "../components/AiProvider";
import type { ChatDetailData } from "../types";
import { ChatDetail } from "./ChatDetail";

const mocks = vi.hoisted(() => ({
  api: {
    aiSettings: vi.fn(),
    chats: vi.fn(),
    chatDetail: vi.fn(),
    summarizeChat: vi.fn(),
    chatTopics: vi.fn(),
    commitments: vi.fn(),
    decisions: vi.fn(),
    onAiIndexProgress: vi.fn(),
    onAiStream: vi.fn(),
  },
}));
vi.mock("../api", () => ({ api: mocks.api }));
vi.mock("../components/Chart", () => ({ Chart: () => <div /> }));

function detail(count: number, nextOffset?: number): ChatDetailData {
  return {
    chat: { id: "team", displayName: "产品群", chatKind: "group", messageCount: count },
    totalMessages: count,
    sentMessages: 1,
    receivedMessages: count - 1,
    firstMessageAt: "2026-10-28T08:00:00+08:00",
    lastMessageAt: "2026-10-28T21:00:00+08:00",
    dailyTrend: [],
    hourlyActivity: [],
    keywords: [],
    messages: Array.from({ length: Math.min(count, 60) }, (_, index) => ({
      localId: count - index,
      occurredAt: "2026-10-28T21:00:00+08:00",
      senderName: "成员",
      isFromMe: false,
      kind: "text",
      text: `消息 ${count - index}`,
    })),
    nextOffset,
  };
}

describe("ChatDetail date focus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetAiState();
    mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
    mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
    mocks.api.onAiStream.mockResolvedValue(() => undefined);
    mocks.api.chats.mockResolvedValue([
      { id: "team", displayName: "产品群", chatKind: "group", messageCount: 80 },
      { id: "amy", displayName: "小艾", chatKind: "direct", messageCount: 4 },
    ]);
    mocks.api.commitments.mockResolvedValue([]);
    mocks.api.decisions.mockResolvedValue([]);
    const older = detail(1);
    older.messages = [{ localId: 20, occurredAt: "2026-10-28T08:00:00+08:00", senderName: "成员", isFromMe: false, kind: "text", text: "消息 20" }];
    older.nextOffset = undefined;
    mocks.api.chatDetail.mockResolvedValueOnce(detail(80, 60)).mockResolvedValue(older);
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("scrolls to the timeline, keeps paging, and does not start AI from a footprint visit", async () => {
    const onReleaseFocus = vi.fn();
    render(
      <AiProvider>
        <ChatDetail accountId="a" range={{ from: "2026-10-28", to: "2026-10-28" }} revision={0} requestedChatId="team" focusTimeline suspendAutoAi onReleaseFocus={onReleaseFocus} onOpenCommitments={() => undefined} />
      </AiProvider>,
    );
    expect(await screen.findByRole("heading", { name: "消息时间线" })).toBeVisible();
    await waitFor(() => expect(Element.prototype.scrollIntoView).toHaveBeenCalled());
    expect(mocks.api.chatDetail).toHaveBeenCalledWith("a", "team", { from: "2026-10-28", to: "2026-10-28" }, 0, 60);
    await waitFor(() => expect(mocks.api.aiSettings).toHaveBeenCalled());
    expect(mocks.api.summarizeChat).not.toHaveBeenCalled();
    expect(mocks.api.chatTopics).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "加载更早消息" }));
    expect(await screen.findByText("消息 20")).toBeVisible();
    expect(mocks.api.chatDetail).toHaveBeenLastCalledWith("a", "team", { from: "2026-10-28", to: "2026-10-28" }, 60, 60);
    fireEvent.click(await screen.findByRole("button", { name: /小艾/ }));
    expect(onReleaseFocus).toHaveBeenCalled();
  });

  it("still loads a cached summary when opened normally", async () => {
    mocks.api.summarizeChat.mockResolvedValue(null);
    mocks.api.chatTopics.mockResolvedValue(null);
    render(
      <AiProvider>
        <ChatDetail accountId="a" range={{}} revision={0} requestedChatId="team" onOpenCommitments={() => undefined} />
      </AiProvider>,
    );
    await screen.findByRole("heading", { name: "消息时间线" });
    await waitFor(() => expect(mocks.api.summarizeChat).toHaveBeenCalled());
    expect(mocks.api.chatTopics).toHaveBeenCalled();
  });
});
