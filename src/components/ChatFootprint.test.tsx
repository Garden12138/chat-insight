import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatFootprintEntry } from "../types";
import { ChatFootprint } from "./ChatFootprint";

const mocks = vi.hoisted(() => ({ api: { chatFootprint: vi.fn() } }));
vi.mock("../api", () => ({ api: mocks.api }));

const rows: ChatFootprintEntry[] = [
  { localDate: "2026-10-28", chatId: "team", displayName: "产品群", chatKind: "group", messageCount: 3, firstMessageAt: "2026-10-28T09:00:00+08:00", lastMessageAt: "2026-10-28T21:00:00+08:00" },
  { localDate: "2026-10-28", chatId: "amy", displayName: "小艾", chatKind: "direct", messageCount: 1, firstMessageAt: "2026-10-28T12:00:00+08:00", lastMessageAt: "2026-10-28T12:30:00+08:00" },
  { localDate: "2026-10-27", chatId: "amy", displayName: "小艾", chatKind: "direct", messageCount: 2, firstMessageAt: "2026-10-27T08:00:00+08:00", lastMessageAt: "2026-10-27T09:00:00+08:00" },
];

describe("ChatFootprint", () => {
  const onOpenChat = vi.fn();
  const onCommitRange = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.api.chatFootprint.mockResolvedValue(rows);
  });

  it("expands the newest day and opens that chat on the selected date", async () => {
    render(<ChatFootprint accountId="a" range={{ from: "2026-10-01", to: "2026-10-28" }} revision={0} onOpenChat={onOpenChat} onCommitRange={onCommitRange} />);
    expect(await screen.findByText("产品群")).toBeVisible();
    expect(screen.getByText("2 个聊天 · 4 条消息")).toBeVisible();
    expect(screen.queryByText("2026-10-27 · 星期二")).not.toBeNull();
    expect(screen.queryByRole("button", { name: /小艾/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /产品群/ }));
    expect(onOpenChat).toHaveBeenCalledWith("team", "2026-10-28");
    expect(mocks.api.chatFootprint).toHaveBeenCalledWith("a", { from: "2026-10-01", to: "2026-10-28" }, undefined);
  });

  it("filters by chat kind and keeps a failed query distinct from an empty day", async () => {
    const view = render(<ChatFootprint accountId="a" range={{ from: "2026-10-01", to: "2026-10-28" }} revision={0} onOpenChat={onOpenChat} onCommitRange={onCommitRange} />);
    await screen.findByText("产品群");
    fireEvent.click(screen.getByRole("button", { name: "群聊" }));
    expect(mocks.api.chatFootprint).toHaveBeenLastCalledWith("a", { from: "2026-10-01", to: "2026-10-28" }, "group");

    mocks.api.chatFootprint.mockRejectedValueOnce(new Error("数据库忙"));
    fireEvent.click(screen.getByRole("button", { name: "私聊" }));
    expect(await screen.findByText("聊天足迹没有加载成功")).toBeVisible();
    expect(screen.getByText("数据库忙")).toBeVisible();
    expect(screen.queryByText("这个日期范围没有聊天记录")).not.toBeInTheDocument();

    mocks.api.chatFootprint.mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByText("这个日期范围没有聊天记录")).toBeVisible();
    view.unmount();
  });

  it("queries only the day chosen in the calendar", async () => {
    render(<ChatFootprint accountId="a" range={{ from: "2026-10-01", to: "2026-10-28" }} revision={0} onOpenChat={onOpenChat} onCommitRange={onCommitRange} />);
    await screen.findByText("产品群");
    fireEvent.click(screen.getByRole("button", { name: "开始日期" }));
    fireEvent.click(screen.getByRole("button", { name: "28" }));
    expect(onCommitRange).toHaveBeenCalledWith({ from: "2026-10-28", to: "2026-10-28" });
    expect(mocks.api.chatFootprint).toHaveBeenLastCalledWith("a", { from: "2026-10-28", to: "2026-10-28" }, undefined);
  });
});
