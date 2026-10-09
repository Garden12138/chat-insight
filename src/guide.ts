import type { ViewName } from "./types";

export const GUIDE_STORAGE_KEY = "chat-insight.guide.completed";

export type GuideMode = "preview" | "tour";

export interface GuideStep {
  id: string;
  title: string;
  body: string;
  /** Page to show behind the spotlight. Preview steps stay on the welcome screen. */
  view?: ViewName;
  /** `data-guide` value of the control this step points at. */
  target?: string;
}

export interface GuideRect {
  top: number;
  left: number;
  width: number;
  height: number;
}

export const guideTarget = {
  range: "range",
  overview: "nav-overview",
  detail: "nav-detail",
  ask: "nav-ask",
  report: "nav-report",
  data: "nav-data",
  privacy: "privacy",
} as const;

/** Read before any dataset exists, from the welcome screen. */
export const previewSteps: GuideStep[] = [
  {
    id: "what",
    title: "这台应用在你的电脑上工作",
    body: "Chat Insight 读取本机的微信聊天，整理成趋势、时间线和搜索。默认不上传内容，也不会改动微信里的消息。下面按第一次使用的顺序说明。",
  },
  {
    id: "import",
    title: "先把聊天放进来",
    body: "支持直接读取时，点「连接本机微信」。Mac 上第一次可能会要求完全磁盘访问、退出并重新打开微信，以及系统管理员授权，欢迎页会先写明。不能直接读取时，用另一台电脑导出的 JSONL，点「导入 Chat Insight 数据」。",
  },
  {
    id: "sync",
    title: "同步会写进本机数据库",
    body: "进度出现后可以取消，已经写入的会留下。每个微信账号单独保存一份。以后换号时，先在微信里登录并点开任意一个聊天，再在「数据管理」里切换。",
  },
  {
    id: "map",
    title: "进来之后按这个顺序看",
    body: "右上角先选时间。总览看整体节奏，聊天详情看某一段关系，搜索与问答找回原话，年度报告看一整年，数据管理负责同步、导出和 AI。进入主界面后，这份指引会再带着真实页面走一遍。",
  },
  {
    id: "ai",
    title: "图表和搜索现在就能用",
    body: "关键词、趋势和年度统计不需要模型。想用摘要、提问或年度叙述时，到「数据管理 → AI 设置」里打开，默认连接本机模型。侧栏底部会显示数据是否留在这台电脑上。",
  },
];

/** Spotlight tour shown the first time the main window has data. */
export const tourSteps: GuideStep[] = [
  {
    id: "welcome",
    title: "用一分钟熟悉这五个页面",
    body: "聊天已经在这台电脑上。指引会按使用顺序走一遍：时间范围、总览、某段聊天、搜索、年度报告，以及数据放在哪里。可以跳过，以后在侧栏底部「使用指引」再打开。",
  },
  {
    id: "range",
    view: "overview",
    target: guideTarget.range,
    title: "先决定看哪一段时间",
    body: "「近 30 天」看最近，「近 1 年」看这一年前后，「全部」适合第一次摸清有多少记录，也可以自己选起止日期。总览、聊天详情和搜索会跟着变。年度报告按年份另选。",
  },
  {
    id: "overview",
    view: "overview",
    target: guideTarget.overview,
    title: "总览：先看聊天的整体样子",
    body: "四个数字是消息总数、活跃天数、聊天数量，以及你发出的比例。点「最常聊天」里的名字，会打开那段对话。热力图表示一周里哪些小时最热闹。",
  },
  {
    id: "detail",
    view: "detail",
    target: guideTarget.detail,
    title: "聊天详情：进入某一段关系",
    body: "左侧搜索好友或群。右侧是这段时间的时间线，图片、视频和表情包可以直接看。同一页还有趋势、活跃时段和关键词；打开 AI 后，还可以生成摘要和常聊话题。",
  },
  {
    id: "ask",
    view: "ask",
    target: guideTarget.ask,
    title: "搜索与问答：找回一句原话",
    body: "只记得几个词时，用关键词搜索，再打开前后文。打开 AI 之后，可以改成直接提问，回答会标出引用了哪几段，点出处回到原消息。",
  },
  {
    id: "report",
    view: "report",
    target: guideTarget.report,
    title: "年度报告：把一年放在一起",
    body: "选择年份后，可以看到全年消息、和上一年的对比、年度聊天排行，以及一段关系是在变多还是变少。这些数字随时可看。文字叙述要等 AI 打开后再生成。",
  },
  {
    id: "data",
    view: "data",
    target: guideTarget.data,
    title: "数据管理：同步、换号、带走",
    body: "微信里有新消息时，点「立即同步」或右上角的刷新。切换账号前，先在微信登录那个号并点开任意聊天。右上角可以在已保存的数据集之间切换。导出的 JSONL 含聊天正文。清空缓存只删除这里的副本。",
  },
  {
    id: "privacy",
    view: "overview",
    target: guideTarget.privacy,
    title: "这里会告诉你数据留在哪里",
    body: "侧栏底部显示「仅在本机」时，分析都在这台电脑上完成。若你启用了云端 AI，这里会改成「已启用云端 AI」，每次发送前仍会再确认一次。",
  },
];

export function stepsFor(mode: GuideMode): GuideStep[] {
  return mode === "preview" ? previewSteps : tourSteps;
}

export function guideCompleted(): boolean {
  try {
    return localStorage.getItem(GUIDE_STORAGE_KEY) === "1";
  } catch {
    return true;
  }
}

export function markGuideCompleted(): void {
  try {
    localStorage.setItem(GUIDE_STORAGE_KEY, "1");
  } catch {
    // Private mode can reject storage; the tour can still be closed for this launch.
  }
}

export function placeGuideCard(
  rect: GuideRect,
  viewport: { width: number; height: number },
  card: { width: number; height: number } = { width: 400, height: 340 },
): { left: number; top: number } {
  const margin = 16;
  const gap = 18;
  const width = Math.min(card.width, viewport.width - margin * 2);
  let left = rect.left + rect.width + gap;
  let top = Math.max(margin, rect.top);
  if (left + width > viewport.width - margin) {
    const beside = rect.left - gap - width;
    if (beside >= margin) left = beside;
    else {
      left = Math.min(Math.max(margin, rect.left), Math.max(margin, viewport.width - width - margin));
      top = rect.top + rect.height + gap;
    }
  }
  if (top + card.height > viewport.height - margin) {
    top = Math.max(margin, viewport.height - card.height - margin);
  }
  return { left, top };
}
