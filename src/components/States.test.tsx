import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EmptyState, InlineNotice } from "./States";

describe("state components", () => {
  it("renders a recoverable empty state", () => {
    render(<EmptyState title="还没有消息" description="请先导入数据" action={<button>选择文件</button>} />);
    expect(screen.getByRole("heading", { name: "还没有消息" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "选择文件" })).toBeInTheDocument();
  });

  it("exposes warning content", () => {
    render(<InlineNotice tone="warning">需要本机授权</InlineNotice>);
    expect(screen.getByText("需要本机授权")).toBeVisible();
  });
});
