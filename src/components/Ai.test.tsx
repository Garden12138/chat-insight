import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { resetAiState, useAi } from "../ai";
import { defaultAiSettings } from "../aiDefaults";
import type { AiSettingsView } from "../types";
import { AiProvider } from "./AiProvider";
import { AiSettings } from "./AiSettings";
import { Markdown } from "./Markdown";
import { TopicsPanel } from "./TopicsPanel";

const mocks = vi.hoisted(() => ({
  api: {
    aiSettings: vi.fn(),
    saveAiSettings: vi.fn(),
    testAi: vi.fn(),
    xcliPrivacy: vi.fn(),
    aiIndexStatus: vi.fn(),
    onAiIndexProgress: vi.fn(),
    chatTopics: vi.fn(),
    onAiStream: vi.fn(),
  },
}));
vi.mock("../api", () => ({ api: mocks.api }));

const cloudSettings: AiSettingsView = {
  ...defaultAiSettings,
  enabled: true,
  baseUrl: "https://api.example.com/v1",
  allowCloud: true,
  isLocal: false,
  host: "api.example.com",
};

beforeEach(() => {
  vi.clearAllMocks();
  resetAiState();
  mocks.api.aiSettings.mockResolvedValue(defaultAiSettings);
  mocks.api.onAiIndexProgress.mockResolvedValue(() => undefined);
  mocks.api.onAiStream.mockResolvedValue(() => undefined);
});

describe("Markdown", () => {
  it("renders headings, lists and clickable citations", () => {
    const onCitation = vi.fn();
    render(<Markdown text={"总体还不错 [1]\n\n### 主要话题\n- **周末** 去公园 [2, 3]\n- 吃火锅"} onCitation={onCitation} />);
    expect(screen.getByRole("heading", { name: "主要话题" })).toBeVisible();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText("周末").tagName).toBe("STRONG");
    fireEvent.click(screen.getByRole("button", { name: "查看来源 3" }));
    expect(onCitation).toHaveBeenCalledWith(3);
  });
});

function ConsentProbe() {
  const { confirmCloud } = useAi();
  const [result, setResult] = useState<string>("pending");
  return (
    <button type="button" onClick={async () => setResult(String(await confirmCloud({ title: "发送聊天记录以生成摘要", detail: "120 条文本消息" })))}>
      ask:{result}
    </button>
  );
}

describe("cloud consent", () => {
  it("treats a loopback xcli gateway as cloud until both selected model routes are verified local", async () => {
    mocks.api.aiSettings.mockResolvedValue({ ...cloudSettings, provider: "xcli", baseUrl: "http://127.0.0.1:18741/v1", host: "127.0.0.1", isLocal: true });
    mocks.api.xcliPrivacy.mockRejectedValue(new Error("catalog unavailable"));
    render(<AiProvider><ConsentProbe /></AiProvider>);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "ask:pending" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("xcli 配置的上游模型");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(await screen.findByRole("button", { name: "ask:null" })).toBeVisible();
    mocks.api.xcliPrivacy.mockResolvedValue({ chatCloud: false, embeddingCloud: false, chatLocation: "local", embeddingLocation: "local" });
    fireEvent.click(screen.getByRole("button", { name: "ask:null" }));
    expect(await screen.findByRole("button", { name: "ask:false" })).toBeVisible();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("does not infer local inference from malformed or partially remote gateway metadata", async () => {
    mocks.api.aiSettings.mockResolvedValue({ ...cloudSettings, provider: "xcli", baseUrl: "http://127.0.0.1:18741/v1" });
    mocks.api.xcliPrivacy.mockResolvedValue({});
    render(<AiProvider><ConsentProbe /></AiProvider>);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "ask:pending" }));
    expect(await screen.findByRole("alertdialog")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await screen.findByRole("button", { name: "ask:null" });
    mocks.api.xcliPrivacy.mockResolvedValue({ chatCloud: false, embeddingCloud: true });
    fireEvent.click(screen.getByRole("button", { name: "ask:null" }));
    expect(await screen.findByRole("alertdialog")).toBeVisible();
  });
  it("resolves immediately for local models", async () => {
    render(<AiProvider><ConsentProbe /></AiProvider>);
    fireEvent.click(screen.getByRole("button", { name: "ask:pending" }));
    expect(await screen.findByRole("button", { name: "ask:false" })).toBeVisible();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("asks before sending chat content to 阿里云百炼", async () => {
    mocks.api.aiSettings.mockResolvedValue({
      ...cloudSettings,
      provider: "bailian",
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      host: "dashscope.aliyuncs.com",
    });
    render(<AiProvider><ConsentProbe /></AiProvider>);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "ask:pending" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("阿里云百炼");
    expect(dialog).not.toHaveTextContent("dashscope.aliyuncs.com");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(await screen.findByRole("button", { name: "ask:null" })).toBeVisible();
  });

  it("asks before sending chat content to a cloud host", async () => {
    mocks.api.aiSettings.mockResolvedValue(cloudSettings);
    render(<AiProvider><ConsentProbe /></AiProvider>);
    await act(async () => {});

    fireEvent.click(screen.getByRole("button", { name: "ask:pending" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("api.example.com");
    expect(dialog).toHaveTextContent("120 条文本消息");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(await screen.findByRole("button", { name: "ask:null" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "ask:null" }));
    fireEvent.click(await screen.findByRole("button", { name: "确认发送" }));
    expect(await screen.findByRole("button", { name: "ask:true" })).toBeVisible();
  });
});

describe("TopicsPanel", () => {
  it("keeps the local keyword cloud while AI is off", () => {
    render(<TopicsPanel accountId="a" chatId="c" chatName="小艾" range={{}} revision={0} keywords={[{ word: "公园", count: 3 }]} />);
    expect(screen.getByRole("heading", { name: "常聊关键词" })).toBeVisible();
    expect(screen.getByText("公园")).toBeVisible();
    expect(mocks.api.chatTopics).not.toHaveBeenCalled();
  });

  it("shows cached topics when AI is on", async () => {
    mocks.api.aiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
    mocks.api.chatTopics.mockResolvedValue({
      topics: [{ name: "周末出游", description: "约时间拍照", share: 0.6, chunkCount: 6, firstAt: "2026-08-01T09:00:00+08:00", lastAt: "2026-09-01T09:00:00+08:00", sample: "" }],
      chunkCount: 10,
      cached: true,
      generatedAt: "2026-10-01T09:00:00+08:00",
    });
    render(<AiProvider><TopicsPanel accountId="a" chatId="c" chatName="小艾" range={{}} revision={0} keywords={[]} /></AiProvider>);
    expect(await screen.findByText("周末出游")).toBeVisible();
    expect(screen.getByText("60%")).toBeVisible();
    expect(mocks.api.chatTopics).toHaveBeenCalledWith("a", "c", {}, expect.any(String), "cached");
  });
});

describe("AiSettings", () => {
  it("selects 阿里云百炼 with an editable namespace endpoint and required API key", async () => {
    render(<AiProvider><AiSettings accountId="a" /></AiProvider>);
    await act(async () => {});
    fireEvent.click(screen.getByRole("radio", { name: "阿里云百炼" }));
    const endpoint = screen.getByLabelText("服务地址");
    expect(endpoint).toHaveValue("https://dashscope.aliyuncs.com/compatible-mode/v1");
    expect(endpoint).not.toHaveAttribute("readonly");
    fireEvent.change(endpoint, { target: { value: "https://llm-demo.cn-beijing.maas.aliyuncs.com/compatible-mode/v1" } });
    expect(endpoint).toHaveValue("https://llm-demo.cn-beijing.maas.aliyuncs.com/compatible-mode/v1");
    expect(screen.getByLabelText("对话模型")).toHaveValue("qwen-plus");
    expect(screen.getByLabelText("向量模型")).toHaveValue("text-embedding-v3");
    expect(screen.getByText("聊天内容会发送到阿里云百炼")).toBeVisible();
    expect(screen.getByLabelText("阿里云百炼 API Key")).toHaveAttribute("placeholder", "必填");
    expect(screen.queryByText(/所有计算都在这台电脑上完成/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/同步后自动找出新约定/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText("允许使用云端模型服务")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存并测试连接" })).toBeEnabled();
    expect(mocks.api.testAi).not.toHaveBeenCalled();
  });
  it("requires opting in before testing a cloud endpoint", async () => {
    render(<AiProvider><AiSettings accountId="a" /></AiProvider>);
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("服务地址"), { target: { value: "https://api.example.com/v1" } });
    expect(screen.getByText("这个地址不在本机")).toBeVisible();
    const testButton = screen.getByRole("button", { name: "保存并测试连接" });
    expect(testButton).toBeDisabled();
    fireEvent.click(screen.getByLabelText("允许使用云端模型服务"));
    expect(testButton).toBeEnabled();
  });

  it("saves without resending an untouched API key", async () => {
    mocks.api.saveAiSettings.mockResolvedValue({ ...defaultAiSettings, enabled: true });
    mocks.api.aiIndexStatus.mockResolvedValue({ model: "bge-m3", running: false, indexedChunks: 0, pendingMessages: 5, totalTextMessages: 5 });
    render(<AiProvider><AiSettings accountId="a" /></AiProvider>);
    await act(async () => {});
    fireEvent.click(screen.getByLabelText(/开启 AI 功能/));
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() => expect(mocks.api.saveAiSettings).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, apiKey: undefined })));
    await waitFor(() => expect(screen.getByRole("button", { name: "建立索引" })).toBeEnabled());
  });
});
