import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MessageItem } from "../types";
import { Lightbox } from "./Lightbox";
import { MediaBubble } from "./MediaBubble";

const mocks = vi.hoisted(() => ({ api: { mediaInfo: vi.fn() } }));
vi.mock("../api", () => ({ api: mocks.api }));

const message = (localId: number, kind: string): MessageItem => ({
  localId,
  occurredAt: "2026-10-08T15:13:35+08:00",
  senderName: "小林",
  isFromMe: false,
  kind,
  text: kind === "video" ? "[视频]" : "[图片]",
});

describe("MediaBubble", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows a loading tile until the media is resolved", () => {
    mocks.api.mediaInfo.mockReturnValue(new Promise(() => {}));
    render(<MediaBubble accountId="a" chatId="c" message={message(1, "image")} onOpen={() => {}} />);
    expect(screen.getByRole("status", { name: "正在加载图片" })).toBeInTheDocument();
  });

  it("opens an available image", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: true, video: false, poster: true });
    const onOpen = vi.fn();
    render(<MediaBubble accountId="a" chatId="c" message={message(1, "image")} onOpen={onOpen} />);
    fireEvent.click(await screen.findByRole("button", { name: "查看图片" }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(mocks.api.mediaInfo).toHaveBeenCalledWith("a", "c", 1, "image");
  });

  it("renders stickers inline without opening the viewer", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: true, video: false, poster: true });
    render(<MediaBubble accountId="a" chatId="c" message={message(5, "sticker")} onOpen={() => {}} />);
    const sticker = await screen.findByRole("img", { name: "表情" });
    expect(sticker).toHaveAttribute("src", "chatmedia://localhost/sticker/a/c/5");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(mocks.api.mediaInfo).toHaveBeenCalledWith("a", "c", 5, "sticker");
  });

  it("explains why a sticker cannot be shown", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: false, video: false, poster: false, reason: "这个表情没有可下载的图片" });
    render(<MediaBubble accountId="a" chatId="c" message={message(6, "sticker")} onOpen={() => {}} />);
    expect(await screen.findByText("表情不可预览")).toBeVisible();
    expect(screen.getByText("这个表情没有可下载的图片")).toBeVisible();
  });

  it("offers playback for videos", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: false, video: true, poster: true });
    render(<MediaBubble accountId="a" chatId="c" message={message(2, "video")} onOpen={() => {}} />);
    expect(await screen.findByRole("button", { name: "播放视频" })).toBeVisible();
  });

  it("uses the first video frame when there is no poster", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: false, video: true, poster: false });
    const { container } = render(<MediaBubble accountId="a" chatId="c" message={message(4, "video")} onOpen={() => {}} />);
    await screen.findByRole("button", { name: "播放视频" });
    expect(container.querySelector("video")?.getAttribute("src")).toBe("chatmedia://localhost/video/a/c/4#t=0.1");
  });

  it("explains why media cannot be shown", async () => {
    mocks.api.mediaInfo.mockResolvedValue({ image: false, video: false, poster: false, reason: "只有当前连接的微信账号可以查看图片和视频" });
    render(<MediaBubble accountId="a" chatId="c" message={message(3, "image")} onOpen={() => {}} />);
    expect(await screen.findByText("只有当前连接的微信账号可以查看图片和视频")).toBeVisible();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("Lightbox", () => {
  const items = [message(1, "image"), message(2, "video"), message(3, "image")];

  beforeEach(() => {
    vi.clearAllMocks();
    mocks.api.mediaInfo.mockImplementation(async (_a: string, _c: string, localId: number) => (
      localId === 2 ? { image: false, video: false, poster: true } : { image: true, video: false, poster: true }
    ));
  });

  it("navigates with the arrow keys and closes on Escape", async () => {
    const onIndexChange = vi.fn();
    const onClose = vi.fn();
    render(<Lightbox accountId="a" chatId="c" messages={items} index={1} onIndexChange={onIndexChange} onClose={onClose} />);
    expect(await screen.findByText("视频还没有下载到本机，请先在微信中点开它")).toBeVisible();
    expect(screen.getByText(/2 \/ 3/)).toBeVisible();

    fireEvent.keyDown(window, { key: "ArrowLeft" });
    fireEvent.keyDown(window, { key: "ArrowRight" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onIndexChange.mock.calls).toEqual([[0], [2]]);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("hides navigation at the ends and toggles zoom", async () => {
    render(<Lightbox accountId="a" chatId="c" messages={items} index={0} onIndexChange={() => {}} onClose={() => {}} />);
    const image = await screen.findByRole("img", { name: "小林发送的图片" });
    expect(screen.queryByRole("button", { name: "上一个" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下一个" })).toBeVisible();
    fireEvent.click(image);
    expect(image).toHaveClass("zoomed");
  });
});
