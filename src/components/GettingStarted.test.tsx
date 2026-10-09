import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { GettingStarted } from "./GettingStarted";
import { guideTarget } from "../guide";
import type { ViewName } from "../types";

function Harness({ onClose }: { onClose: () => void }) {
  const [view, setView] = useState<ViewName>("overview");
  return (
    <>
      <h1>{view}</h1>
      <button type="button" data-guide={guideTarget.range}>时间范围</button>
      <button type="button" data-guide={guideTarget.detail}>聊天详情</button>
      <GettingStarted open mode="tour" view={view} onView={setView} onClose={onClose} />
    </>
  );
}

describe("getting started tour", () => {
  it("walks forward, switches pages, and closes on the last step", () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);

    expect(screen.getByRole("dialog", { name: "用一分钟熟悉这五个页面" })).toBeVisible();
    expect(screen.getByRole("button", { name: "上一步" })).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "先决定看哪一段时间" })).toBeVisible();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("overview");

    fireEvent.click(screen.getByRole("button", { name: "上一步" }));
    expect(screen.getByRole("heading", { name: "用一分钟熟悉这五个页面" })).toBeVisible();

    for (let step = 0; step < 3; step += 1) fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "聊天详情：进入某一段关系" })).toBeVisible();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("detail");

    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("finishes the welcome preview without pointing at the main window", () => {
    const onClose = vi.fn();
    render(<GettingStarted open mode="preview" onClose={onClose} />);

    expect(screen.getByRole("dialog", { name: "这台应用在你的电脑上工作" })).toBeVisible();
    expect(document.querySelector(".guide-hole")).toBeNull();

    for (let step = 0; step < 4; step += 1) fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
