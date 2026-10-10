import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { GUIDE_STORAGE_KEY } from "./guide";
import type { AccountSummary, PlatformCapabilities, SyncProgress } from "./types";

const mocks = vi.hoisted(() => ({
  progressHandler: undefined as ((progress: SyncProgress) => void) | undefined,
  api: {
    capabilities: vi.fn(),
    accounts: vi.fn(),
    bootstrap: vi.fn(),
    switchAccount: vi.fn(),
    sync: vi.fn(),
    dashboard: vi.fn(),
    chats: vi.fn(),
    chatDetail: vi.fn(),
    onSyncProgress: vi.fn(),
    commitments: vi.fn(),
    commitmentCounts: vi.fn(),
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
    localStorage.clear();
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

  it("previews how to start before a dataset exists", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "先看使用指引" }));

    expect(screen.getByRole("dialog", { name: "这台应用在你的电脑上工作" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "跳过" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(localStorage.getItem(GUIDE_STORAGE_KEY)).toBeNull();
    expect(screen.getByRole("button", { name: /连接本机微信/ })).toBeVisible();
  });
});

describe("switching WeChat accounts", () => {
  const oldAccount: AccountSummary = {
    id: "wechat:wxid_old_1a2b",
    displayName: "旧号",
    sourcePlatform: "macos",
    messageCount: 10,
    chatCount: 2,
    lastSyncAt: "2026-09-01T00:00:00Z",
  };
  const newAccount: AccountSummary = { ...oldAccount, id: "wechat:wxid_new_3c4d", displayName: "新号" };

  beforeEach(() => {
    localStorage.setItem(GUIDE_STORAGE_KEY, "1");
    vi.clearAllMocks();
    mocks.api.capabilities.mockResolvedValue(macCapabilities);
    mocks.api.accounts.mockResolvedValue([oldAccount]);
    mocks.api.dashboard.mockReturnValue(new Promise(() => {}));
    mocks.api.onSyncProgress.mockResolvedValue(() => undefined);
    mocks.api.commitmentCounts.mockResolvedValue({ suggested: 0, open: 0, overdue: 0, done: 0, dismissed: 0, suggestedTotal: 0, acceptedTotal: 0 });
    mocks.api.commitments.mockResolvedValue([]);
  });

  async function openDataManagement() {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "数据管理" }));
  }

  it("explains that the connected account is still logged in", async () => {
    mocks.api.switchAccount.mockResolvedValue({ ready: true, message: "", switched: false, accountId: oldAccount.id });

    await openDataManagement();
    fireEvent.click(screen.getByRole("button", { name: "切换账号" }));

    expect(await screen.findByText(/微信当前登录的仍是“旧号”/)).toBeVisible();
    expect(mocks.api.sync).not.toHaveBeenCalled();
  });

  it("syncs the newly connected account into its own dataset", async () => {
    mocks.api.switchAccount.mockResolvedValue({ ready: true, message: "", switched: true, accountId: newAccount.id });
    mocks.api.sync.mockResolvedValue({ accountId: newAccount.id, chatsProcessed: 1, messagesImported: 3, warnings: 0, cancelled: false });

    await openDataManagement();
    mocks.api.accounts.mockResolvedValue([newAccount, oldAccount]);
    fireEvent.click(screen.getByRole("button", { name: "切换账号" }));

    expect(await screen.findByText("同步完成：新增 3 条消息")).toBeVisible();
    expect(screen.getByRole("combobox")).toHaveValue(newAccount.id);
    expect(screen.getByRole("heading", { name: "新号" })).toBeVisible();
  });
});

describe("first-run guide", () => {
  const account: AccountSummary = {
    id: "wechat:wxid_old_1a2b",
    displayName: "旧号",
    sourcePlatform: "macos",
    messageCount: 10,
    chatCount: 2,
    lastSyncAt: "2026-09-01T00:00:00Z",
  };

  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    mocks.api.capabilities.mockResolvedValue(macCapabilities);
    mocks.api.accounts.mockResolvedValue([account]);
    mocks.api.dashboard.mockReturnValue(new Promise(() => {}));
    mocks.api.chats.mockResolvedValue([]);
    mocks.api.chatDetail.mockResolvedValue(undefined);
    mocks.api.onSyncProgress.mockResolvedValue(() => undefined);
    mocks.api.commitmentCounts.mockResolvedValue({ suggested: 0, open: 0, overdue: 0, done: 0, dismissed: 0, suggestedTotal: 0, acceptedTotal: 0 });
    mocks.api.commitments.mockResolvedValue([]);
  });

  it("walks a new user through the main pages and can be opened again", async () => {
    render(<App />);

    expect(await screen.findByRole("dialog", { name: "用一分钟熟悉这六个页面" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "约定与待办：别忘了答应过的事" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(screen.getByRole("heading", { name: "聊天详情" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "聊天详情：进入某一段关系" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "跳过" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(localStorage.getItem(GUIDE_STORAGE_KEY)).toBe("1");
    expect(screen.getByRole("heading", { name: "聊天总览" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "使用指引" }));
    expect(screen.getByRole("dialog", { name: "用一分钟熟悉这六个页面" })).toBeVisible();
  });
});
