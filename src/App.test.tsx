import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { PlatformCapabilities, SyncProgress } from "./types";

const mocks = vi.hoisted(() => ({
  progressHandler: undefined as ((progress: SyncProgress) => void) | undefined,
  api: {
    capabilities: vi.fn(),
    accounts: vi.fn(),
    bootstrap: vi.fn(),
    sync: vi.fn(),
    onSyncProgress: vi.fn(),
  },
}));

vi.mock("./api", () => ({ api: mocks.api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const macCapabilities: PlatformCapabilities = {
  os: "macos",
  arch: "aarch64",
  directWechatRead: true,
  bridgeBundled: true,
  setupState: "needs_setup",
  setupHint: "需要本机授权",
};

const preparing: SyncProgress = {
  phase: "preparing",
  current: 0,
  total: 1,
  message: "正在启动本机授权引导…",
  warnings: 0,
  running: true,
};

describe("onboarding local authorization", () => {
  beforeEach(() => {
    mocks.progressHandler = undefined;
    mocks.api.capabilities.mockResolvedValue(macCapabilities);
    mocks.api.accounts.mockResolvedValue([]);
    mocks.api.onSyncProgress.mockImplementation(async (handler: (progress: SyncProgress) => void) => {
      mocks.progressHandler = handler;
      return () => undefined;
    });
  });

  it("replaces the running progress with the failure reason", async () => {
    mocks.api.bootstrap.mockImplementation(async () => {
      act(() => mocks.progressHandler?.(preparing));
      throw "未找到微信应用：请把微信安装在“应用程序”文件夹后重试";
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /连接本机微信/ }));

    expect(await screen.findByText(/未找到微信应用/)).toBeVisible();
    expect(screen.queryByText(preparing.message)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /连接本机微信/ })).toBeEnabled();
  });
});
