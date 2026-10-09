import { isValidElement } from "react";
import { describe, expect, it } from "vitest";
import { emojiText, withEmoji } from "./emoji";

describe("WeChat emoji", () => {
  it("replaces known codes and keeps everything else", () => {
    expect(emojiText("哈哈[捂脸][旺柴] a[0] [不存在]")).toBe("哈哈🤦🐶 a[0] [不存在]");
    expect(emojiText("[破涕為笑][OK]")).toBe("😂👌");
  });

  it("renders codes as labelled emoji spans", () => {
    const parts = withEmoji("好的[玫瑰]谢谢");
    expect(parts[0]).toBe("好的");
    expect(isValidElement(parts[1]) && (parts[1].props as { "aria-label": string })["aria-label"]).toBe("玫瑰");
    expect(parts[2]).toBe("谢谢");
    expect(withEmoji("没有表情")).toEqual(["没有表情"]);
  });
});
