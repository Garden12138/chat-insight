import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetAiState } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import { AiProvider } from "../components/AiProvider";
import type { AiStreamEvent, Commitment, CommitmentFilter, CommitmentScanOutcome } from "../types";
import { toLocalDate } from "../utils";
import { Commitments } from "./Commitments";

const mocks = vi.hoisted(() => ({
  streamHandler: undefined as ((event: AiStreamEvent) => void) | undefined,
  api: {
    aiSettings: vi.fn(),
    chats: vi.fn(),
    commitments: vi.fn(),
    commitmentCounts: vi.fn(),
    updateCommitment: vi.fn(),
    commitmentEstimate: vi.fn(),
    scanCommitments: vi.fn(),
    messageContext: vi.fn(),
    cancelAi: vi.fn(),
    onAiStream: vi.fn(),
    onAiIndexProgress: vi.fn(),
  },
}));
vi.mock("../api", () => ({ api: mocks.api }));

const later = new Date();
later.setDate(later.getDate() + 3);

const suggestion: Commitment = {
  id: 1,
  chatId: "alice",
  chatName: "小艾",
  chatKind: "direct",
  direction: "mine",
  ownerName: "我",
  title: "把合同发给小艾",
  quote: "周五前我把合同发你",
  dueText: "周五前",
  dueDate: toLocalDate(later),
  status: "suggested",
  aiStatus: "done",
  aiNote: "合同已发",
  followupLocalId: 5,
  sourceLocalId: 2,
  sourceEndLocalId: 2,
  sourceAt: "2026-10-12T09:01:00+08:00",
  confidence: 0.9,
  userEdited: false,
  createdAt: "2026-10-12T10:00:00Z",
  updatedAt: "2026-10-12T10:00:00Z",
};

let store: Commitment[];

function renderView(onCountsChange = vi.fn()) {
  render(<AiProvider><Commitments accountId="a" range={{ from: "2026-09-13", to: "2026-10-12" }} revision={0} onCountsChange={onCountsChange} onOpenDetail={vi.fn()} onOpenSettings={vi.fn()} /></AiProvider>);
  return onCountsChange;
}

beforeEach(() => {
  vi.clearAllMocks();
  resetAiState();
  store = [{ ...suggestion }];
  mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
  mocks.api.chats.mockResolvedValue([{ id: "alice", displayName: "小艾", chatKind: "direct", messageCount: 3 }]);
  mocks.api.commitments.mockImplementation(async (_account: string, filter: CommitmentFilter) =>
    store.filter((item) => filter.statuses?.includes(item.status)));
  mocks.api.commitmentCounts.mockImplementation(async () => ({
    suggested: store.filter((item) => item.status === "suggested").length,
    open: store.filter((item) => item.status === "open").length,
    overdue: 0,
    done: store.filter((item) => item.status === "done").length,
    dismissed: store.filter((item) => item.status === "dismissed").length,
    suggestedTotal: store.length,
    acceptedTotal: store.filter((item) => item.status === "open" || item.status === "done").length,
  }));
  mocks.api.updateCommitment.mockImplementation(async (_account: string, id: number, patch: { status?: Commitment["status"]; edit?: { title: string; dueDate?: string } }) => {
    store = store.map((item) => item.id !== id ? item : {
      ...item,
      status: patch.status ?? item.status,
      title: patch.edit?.title ?? item.title,
      dueDate: patch.edit ? patch.edit.dueDate || undefined : item.dueDate,
      userEdited: item.userEdited || !!patch.edit,
    });
    return store.find((item) => item.id === id);
  });
  mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
  mocks.api.onAiStream.mockImplementation(async (handler: (event: AiStreamEvent) => void) => {
    mocks.streamHandler = handler;
    return () => { mocks.streamHandler = undefined; };
  });
});

describe("commitment queue", () => {
  it("confirms a suggestion, then completes and restores it", async () => {
    const onCountsChange = renderView();
    const card = (await screen.findByRole("heading", { name: "把合同发给小艾" })).closest("li") as HTMLElement;
    expect(within(card).getByText("我答应的")).toBeVisible();
    expect(screen.getByText(/还有 3 天/)).toBeVisible();
    expect(screen.getByText(/后续消息显示可能已经完成：合同已发/)).toBeVisible();
    expect(onCountsChange).toHaveBeenLastCalledWith(expect.objectContaining({ suggested: 1 }));

    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    expect(await screen.findByText("没有待确认的约定")).toBeVisible();
    expect(mocks.api.updateCommitment).toHaveBeenCalledWith("a", 1, { status: "open" });

    fireEvent.click(screen.getByRole("tab", { name: /进行中/ }));
    fireEvent.click(await screen.findByRole("button", { name: "完成" }));
    expect(await screen.findByText("没有进行中的约定")).toBeVisible();

    fireEvent.click(screen.getByRole("tab", { name: /已完成或忽略/ }));
    expect(await screen.findByText("已完成")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "恢复" }));
    expect(await screen.findByText("还没有完成或忽略的约定")).toBeVisible();
    expect(screen.getByText(/AI 一共建议了 1 条约定，你保留了 1 条/)).toBeVisible();
  });

  it("dismisses and edits suggestions", async () => {
    store = [{ ...suggestion }, { ...suggestion, id: 2, title: "请小艾吃饭", direction: "theirs", ownerName: "小艾", dueDate: undefined, dueText: undefined, aiStatus: undefined }];
    renderView();
    const card = (await screen.findByRole("heading", { name: "请小艾吃饭" })).closest("li") as HTMLElement;
    expect(within(card).getByText("小艾答应我的")).toBeVisible();

    fireEvent.click(within(card).getByRole("button", { name: "更多操作" }));
    fireEvent.click(within(card).getByRole("menuitem", { name: "编辑" }));
    expect(within(card).queryByRole("button", { name: "更多操作" })).not.toBeInTheDocument();
    fireEvent.change(within(card).getByLabelText("约定内容"), { target: { value: "小艾请我吃饭" } });
    fireEvent.change(within(card).getByLabelText("截止日期"), { target: { value: "2026-10-20" } });
    fireEvent.click(within(card).getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("heading", { name: "小艾请我吃饭" })).toBeVisible();
    expect(mocks.api.updateCommitment).toHaveBeenCalledWith("a", 2, { edit: { title: "小艾请我吃饭", dueDate: "2026-10-20" } });

    const first = screen.getByRole("heading", { name: "把合同发给小艾" }).closest("li") as HTMLElement;
    fireEvent.click(within(first).getByRole("button", { name: "更多操作" }));
    fireEvent.click(within(first).getByRole("menuitem", { name: "忽略" }));
    await act(async () => {});
    expect(screen.queryByRole("heading", { name: "把合同发给小艾" })).not.toBeInTheDocument();
  });

  it("opens the original message", async () => {
    mocks.api.messageContext.mockResolvedValue([
      { localId: 2, occurredAt: suggestion.sourceAt, senderName: "", isFromMe: true, kind: "text", text: "周五前我把合同发你" },
    ]);
    renderView();
    fireEvent.click(await screen.findByRole("button", { name: "查看原话" }));
    expect(await screen.findByRole("dialog", { name: "小艾 的聊天上下文" })).toBeVisible();
    expect(mocks.api.messageContext).toHaveBeenCalledWith("a", "alice", 2, 8);
  });
});

describe("scanning", () => {
  it("scans with the local model and reports what it found", async () => {
    let finish: (value: CommitmentScanOutcome) => void = () => undefined;
    mocks.api.scanCommitments.mockImplementation((_account: string, _range: unknown, _chat: unknown, requestId: string) => {
      act(() => mocks.streamHandler?.({ requestId, delta: "", status: "正在阅读“小艾”（第 1/2 段）…" }));
      return new Promise<CommitmentScanOutcome>((resolve) => { finish = resolve; });
    });
    renderView();
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "扫描聊天" }));
    expect(await screen.findByText("正在阅读“小艾”（第 1/2 段）…")).toBeVisible();
    expect(mocks.api.scanCommitments).toHaveBeenCalledWith("a", { from: "2026-09-13", to: "2026-10-12" }, "", expect.any(String), false);
    expect(mocks.api.commitmentEstimate).not.toHaveBeenCalled();

    await act(async () => finish({ chats: 1, windows: 2, skipped: 3, failed: 0, created: 1, updated: 0, cancelled: false }));
    expect(screen.getByText("扫描了 2 段聊天，跳过 3 段没有约定迹象的聊天，新增 1 条建议。")).toBeVisible();
  });

  it("asks before sending to a cloud model and keeps saved items without AI", async () => {
    mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true, isLocal: false, allowCloud: true, host: "api.example.com" });
    mocks.api.commitmentEstimate.mockResolvedValue({ chats: 2, windows: 4, skipped: 9, chars: 3200 });
    renderView();
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "扫描聊天" }));
    expect(await screen.findByText(/大约 4 段聊天/)).toBeVisible();
    expect(mocks.api.scanCommitments).not.toHaveBeenCalled();
  });

  it("explains that scanning needs AI", async () => {
    mocks.api.aiSettings.mockResolvedValue(defaultAiSettings);
    renderView();
    expect(await screen.findByText("扫描需要先开启 AI")).toBeVisible();
    expect(screen.queryByRole("button", { name: "扫描聊天" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "把合同发给小艾" })).toBeVisible();
  });
});
