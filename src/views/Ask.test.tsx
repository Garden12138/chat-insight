import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetAiState } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import { AiProvider } from "../components/AiProvider";
import type { AiStreamEvent, AskResult, EventReviewEstimate, EventReviewResult, SearchHit } from "../types";
import { Ask } from "./Ask";

const mocks = vi.hoisted(() => ({
  streamHandler: undefined as ((event: AiStreamEvent) => void) | undefined,
  api: {
    aiSettings: vi.fn(),
    chats: vi.fn(),
    aiIndexStatus: vi.fn(),
    searchMessages: vi.fn(),
    messageContext: vi.fn(),
    ask: vi.fn(),
    eventReviewEstimate: vi.fn(),
    chatFootprint: vi.fn(),
    reviewGroupEvents: vi.fn(),
    cancelAi: vi.fn(),
    onAiStream: vi.fn(),
    onAiIndexProgress: vi.fn(),
  },
}));
vi.mock("../api", () => ({ api: mocks.api }));

const hit: SearchHit = {
  chatId: "alice",
  chatName: "小艾",
  chatKind: "direct",
  localId: 7,
  occurredAt: "2026-08-20T09:00:00+08:00",
  senderName: "小艾",
  isFromMe: false,
  kind: "text",
  text: "周末一起去公园拍照",
};

function renderAsk(onOpenDetail = vi.fn()) {
  const view = render(<AiProvider><Ask accountId="a" range={{}} revision={0} onOpenDetail={onOpenDetail} onOpenSettings={() => {}} /></AiProvider>);
  return Object.assign(onOpenDetail, { unmount: view.unmount });
}

beforeEach(() => {
  vi.clearAllMocks();
  resetAiState();
  mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
  mocks.streamHandler = undefined;
  mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
  mocks.api.chats.mockResolvedValue([
    { id: "alice", displayName: "小艾", chatKind: "direct", messageCount: 3 },
    { id: "g1", displayName: "产品群", chatKind: "group", messageCount: 12 },
  ]);
  mocks.api.aiIndexStatus.mockResolvedValue({ model: "bge-m3", running: false, indexedChunks: 4, pendingMessages: 0, totalTextMessages: 9 });
  mocks.api.eventReviewEstimate.mockResolvedValue({ date: "2026-10-27", chatCount: 1, messageCount: 4, chars: 80, segmentCount: 1 } satisfies EventReviewEstimate);
  mocks.api.chatFootprint.mockResolvedValue([]);
  mocks.api.cancelAi.mockResolvedValue(undefined);
  mocks.api.onAiStream.mockImplementation(async (handler: (event: AiStreamEvent) => void) => {
    mocks.streamHandler = handler;
    return () => { mocks.streamHandler = undefined; };
  });
});

describe("keyword search", () => {
  it("highlights hits and opens their context", async () => {
    mocks.api.searchMessages.mockResolvedValue([hit]);
    mocks.api.messageContext.mockResolvedValue([
      { localId: 6, occurredAt: hit.occurredAt, senderName: "", isFromMe: true, kind: "text", text: "这周有空吗" },
      { localId: 7, occurredAt: hit.occurredAt, senderName: "小艾", isFromMe: false, kind: "text", text: hit.text },
    ]);
    const onOpenDetail = renderAsk();
    await act(async () => {});

    fireEvent.click(screen.getByRole("tab", { name: /关键词搜索/ }));
    fireEvent.change(screen.getByLabelText("输入关键词"), { target: { value: "公园" } });
    fireEvent.click(screen.getByRole("button", { name: "搜索" }));

    expect(await screen.findByText("找到 1 条消息")).toBeVisible();
    expect(screen.getByText("公园").tagName).toBe("MARK");
    expect(mocks.api.searchMessages).toHaveBeenCalledWith("a", "公园", {}, "", 200);

    fireEvent.click(screen.getByRole("button", { name: /周末一起去/ }));
    expect(await screen.findByText("这周有空吗")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "在聊天详情中打开" }));
    expect(onOpenDetail).toHaveBeenCalledWith("alice");
  });
});

describe("AI ask", () => {
  it("streams the answer and lists cited sources", async () => {
    const result: AskResult = {
      answer: "你们约了周末去公园 [1]",
      sources: [{ index: 1, chatId: "alice", chatName: "小艾", chatKind: "direct", localId: 7, endLocalId: 8, startAt: hit.occurredAt, endAt: hit.occurredAt, text: "小艾: 周末一起去公园拍照" }],
    };
    let finish: (value: AskResult) => void = () => undefined;
    mocks.api.ask.mockImplementation((_account: string, _question: string, _range: unknown, _chat: unknown, requestId: string) => {
      act(() => mocks.streamHandler?.({ requestId, delta: "你们约了", status: undefined }));
      return new Promise<AskResult>((resolve) => { finish = resolve; });
    });
    renderAsk();
    await act(async () => {});

    fireEvent.change(screen.getByLabelText("输入问题"), { target: { value: "我们约了去哪？" } });
    fireEvent.click(screen.getByRole("button", { name: "提问" }));

    expect(await screen.findByText("你们约了")).toBeVisible();
    expect(screen.getByRole("button", { name: "停止" })).toBeVisible();
    expect(mocks.api.ask).toHaveBeenCalledWith("a", "我们约了去哪？", {}, "", expect.any(String), false);

    await act(async () => finish(result));
    expect(screen.getByRole("button", { name: "查看来源 1" })).toBeVisible();
    expect(screen.getByText("小艾: 周末一起去公园拍照")).toBeVisible();
    expect(screen.getByRole("button", { name: "提问" })).toBeVisible();
  });

  it("keeps a running question and its answer across page switches", async () => {
    let finish: (value: AskResult) => void = () => undefined;
    mocks.api.ask.mockImplementation((_account: string, _question: string, _range: unknown, _chat: unknown, requestId: string) => {
      act(() => mocks.streamHandler?.({ requestId, delta: "正在找", status: undefined }));
      return new Promise<AskResult>((resolve) => { finish = resolve; });
    });
    const first = renderAsk();
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("输入问题"), { target: { value: "周末去哪？" } });
    fireEvent.click(screen.getByRole("button", { name: "提问" }));
    expect(await screen.findByText("正在找")).toBeVisible();

    first.unmount();
    renderAsk();
    await act(async () => {});
    expect(screen.getByRole("heading", { name: "周末去哪？" })).toBeVisible();
    expect(screen.getByLabelText("输入问题")).toHaveValue("周末去哪？");
    expect(screen.getByRole("button", { name: "停止" })).toBeVisible();

    await act(async () => finish({ answer: "去公园 [捂脸]", sources: [] }));
    expect(screen.getByText("去公园")).toBeVisible();
    expect(screen.getByRole("img", { name: "捂脸" })).toHaveTextContent("🤦");
    expect(mocks.api.ask).toHaveBeenCalledOnce();
  });

  it("points to settings when AI is off", async () => {
    mocks.api.aiSettings.mockResolvedValue(defaultAiSettings);
    renderAsk();
    await act(async () => {});
    fireEvent.click(screen.getByRole("tab", { name: /AI 问答/ }));
    expect(screen.getByText("AI 问答尚未开启")).toBeVisible();
    expect(screen.getByRole("button", { name: "提问" })).toBeDisabled();
  });
});

const reviewResult: EventReviewResult = {
  date: "2026-10-27",
  scope: "all-chats",
  events: [{
    id: "g1:1:发布",
    title: "发布窗口调整",
    category: "decision",
    summary: "今晚发布",
    startAt: "2026-10-27T09:00:00+08:00",
    endAt: "2026-10-27T09:00:00+08:00",
    sourceRefs: [{
      chatId: "g1",
      chatName: "产品群",
      chatKind: "group",
      localId: 1,
      occurredAt: "2026-10-27T09:00:00+08:00",
      senderName: "成员",
      isFromMe: false,
      text: "发布窗口改为今晚",
    }],
  }],
  chatCount: 1,
  messageCount: 4,
  processedMessages: 4,
  totalSegments: 1,
  completedSegments: 1,
  failedSegments: 0,
  complete: true,
  cancelled: false,
  cached: false,
};

describe("event review", () => {
  it("routes an exact date recap to event review and opens a real source", async () => {
    mocks.api.reviewGroupEvents.mockResolvedValue(reviewResult);
    mocks.api.messageContext.mockResolvedValue([
      { localId: 1, occurredAt: reviewResult.events[0].startAt, senderName: "成员", isFromMe: false, kind: "text", text: "发布窗口改为今晚" },
    ]);
    renderAsk();
    await act(async () => {});

    fireEvent.change(screen.getByLabelText("输入问题"), { target: { value: "2026-10-27发生了什么" } });
    fireEvent.click(screen.getByRole("button", { name: "提问" }));

    expect(await screen.findByText("已识别日期 2026年10月27日，已切换到事件回顾")).toBeVisible();
    expect(screen.getByRole("tab", { name: "事件回顾" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "AI 事件回顾" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByText("发布窗口调整")).toBeVisible();
    expect(mocks.api.ask).not.toHaveBeenCalled();
    expect(mocks.api.reviewGroupEvents).toHaveBeenCalledWith("a", "2026-10-27", undefined, expect.any(String), "generate", false);

    fireEvent.click(screen.getByRole("button", { name: /查看原话/ }));
    expect(await screen.findByText("发布窗口改为今晚")).toBeVisible();
  });

  it("routes a private-chat recap to that chat instead of all chats", async () => {
    mocks.api.reviewGroupEvents.mockResolvedValue({
      ...reviewResult,
      scope: "alice",
      chatCount: 1,
      events: [{
        ...reviewResult.events[0],
        id: "alice:7:约拍照",
        title: "约周末拍照",
        sourceRefs: [{
          chatId: "alice",
          chatName: "小艾",
          chatKind: "direct",
          localId: 7,
          occurredAt: hit.occurredAt,
          senderName: "小艾",
          isFromMe: false,
          text: "周末一起去公园拍照",
        }],
      }],
    });
    renderAsk();
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("搜索范围"), { target: { value: "alice" } });
    fireEvent.change(screen.getByLabelText("输入问题"), { target: { value: "2026-10-27发生了什么" } });
    fireEvent.click(screen.getByRole("button", { name: "提问" }));
    expect(await screen.findByText("已识别日期 2026年10月27日，已切换到事件回顾")).toBeVisible();
    expect(await screen.findByText("约周末拍照")).toBeVisible();
    expect(mocks.api.ask).not.toHaveBeenCalled();
    expect(mocks.api.reviewGroupEvents).toHaveBeenCalledWith("a", "2026-10-27", "alice", expect.any(String), "generate", false);
  });

  it("lists private chats in the review scope", async () => {
    renderAsk();
    await act(async () => {});
    fireEvent.click(screen.getByRole("tab", { name: "事件回顾" }));
    fireEvent.click(screen.getByRole("tab", { name: "AI 事件回顾" }));
    const scope = screen.getByLabelText("分析范围");
    expect(scope).toHaveDisplayValue("全部聊天");
    fireEvent.change(scope, { target: { value: "alice" } });
    expect(scope).toHaveValue("alice");
    expect(screen.getByRole("option", { name: "小艾" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "产品群" })).toBeInTheDocument();
  });

  it("shows an empty-message state without calling the model", async () => {
    mocks.api.eventReviewEstimate.mockResolvedValue({ date: "2026-10-27", chatCount: 0, messageCount: 0, chars: 0, segmentCount: 0 });
    renderAsk();
    await act(async () => {});
    fireEvent.click(screen.getByRole("tab", { name: "事件回顾" }));
    fireEvent.click(screen.getByRole("tab", { name: "AI 事件回顾" }));
    fireEvent.click(screen.getByRole("button", { name: "回顾日期" }));
    fireEvent.click(screen.getByRole("button", { name: "27" }));
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "开始分析" }));
    expect(await screen.findByText("这一天没有可分析的聊天文本")).toBeVisible();
    expect(mocks.api.reviewGroupEvents).not.toHaveBeenCalled();
  });

  it("keeps a factual dated question in ordinary ask", async () => {
    mocks.api.ask.mockResolvedValue({ answer: "小王处理了接口", sources: [] });
    renderAsk();
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("输入问题"), { target: { value: "2026年10月27日谁处理了接口问题" } });
    fireEvent.click(screen.getByRole("button", { name: "提问" }));
    expect(await screen.findByText("小王处理了接口")).toBeVisible();
    expect(mocks.api.reviewGroupEvents).not.toHaveBeenCalled();
  });
});
