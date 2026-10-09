import { describe, expect, it } from "vitest";
import { isMediaKind, isPreviewKind, mediaUrl } from "./media";

describe("mediaUrl", () => {
  it("encodes every segment for the chatmedia protocol", () => {
    expect(mediaUrl("video", "wechat:wxid_a_1234", "4511@chatroom", 3032, false))
      .toBe("chatmedia://localhost/video/wechat%3Awxid_a_1234/4511%40chatroom/3032");
  });

  it("uses the localhost form on Windows", () => {
    expect(mediaUrl("image", "a", "b/c", 1, true)).toBe("http://chatmedia.localhost/image/a/b%2Fc/1");
  });

  it("recognizes previewable kinds", () => {
    expect(isMediaKind("image")).toBe(true);
    expect(isMediaKind("video")).toBe(true);
    expect(isMediaKind("text")).toBe(false);
    expect(isMediaKind("sticker")).toBe(false);
    expect(isPreviewKind("sticker")).toBe(true);
    expect(isPreviewKind("voice")).toBe(false);
  });
});
