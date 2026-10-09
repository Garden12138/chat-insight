import { createElement, type ReactNode } from "react";

/** WeChat's built-in emoji are stored as `[名称]` in message text; these are the closest Unicode equivalents. */
export const WECHAT_EMOJI: Record<string, string> = {
  微笑: "🙂", 撇嘴: "😕", 色: "😍", 发呆: "😳", 得意: "😎", 流泪: "😢", 害羞: "😊", 闭嘴: "🤐",
  睡: "😴", 大哭: "😭", 尴尬: "😅", 发怒: "😡", 调皮: "😜", 呲牙: "😁", 惊讶: "😮", 难过: "🙁",
  酷: "😎", 冷汗: "😰", 抓狂: "😫", 吐: "🤮", 偷笑: "🤭", 愉快: "😄", 可爱: "😊", 白眼: "🙄",
  傲慢: "😤", 饥饿: "😋", 困: "😪", 惊恐: "😱", 流汗: "😓", 汗: "😓", 憨笑: "😄", 悠闲: "😌",
  奋斗: "💪", 咒骂: "🤬", 疑问: "❓", 嘘: "🤫", 晕: "😵", 疯了: "🤪", 折磨: "😖", 衰: "😞",
  骷髅: "💀", 敲打: "🔨", 再见: "👋", 擦汗: "😥", 抠鼻: "🤥", 鼓掌: "👏", 糗大了: "😳", 坏笑: "😏",
  左哼哼: "😤", 右哼哼: "😤", 哈欠: "🥱", 鄙视: "😒", 委屈: "🥺", 快哭了: "🥹", 阴险: "😈", 亲亲: "😘",
  吓: "😨", 可怜: "🥺", 笑脸: "😃", 生病: "😷", 脸红: "😳", 破涕为笑: "😂", 恐惧: "😨", 失望: "😞",
  无语: "😑", 嘿哈: "😆", 捂脸: "🤦", 奸笑: "😏", 机智: "🤓", 皱眉: "😟", 耶: "✌️", 吃瓜: "🍉",
  加油: "💪", 天啊: "😱", Emm: "🤔", 社会社会: "🤙", 旺柴: "🐶", 好的: "👌", 打脸: "🤕", 哇: "🤩",
  翻白眼: "🙄", "666": "🤙", 让我看看: "👀", 叹气: "😮‍💨", 苦涩: "😣", 裂开: "🫠", 囧: "😳", 掩面: "🫣",
  吼嘿: "😆", 难受: "😖", 汗颜: "😓", doge: "🐶", 二哈: "🐶",
  菜刀: "🔪", 西瓜: "🍉", 吃西瓜: "🍉", 啤酒: "🍺", 篮球: "🏀", 乒乓: "🏓", 咖啡: "☕", 茶: "🍵",
  饭: "🍚", 猪头: "🐷", 玫瑰: "🌹", 凋谢: "🥀", 嘴唇: "💋", 爱心: "❤️", 心碎: "💔", 蛋糕: "🎂",
  闪电: "⚡", 炸弹: "💣", 刀: "🗡️", 足球: "⚽", 瓢虫: "🐞", 便便: "💩", 月亮: "🌙", 太阳: "☀️",
  礼物: "🎁", 拥抱: "🤗", 强: "👍", 弱: "👎", 握手: "🤝", 胜利: "✌️", 抱拳: "🙏", 勾引: "👉",
  拳头: "👊", 差劲: "👎", 爱你: "🤟", NO: "🙅", OK: "👌", 爱情: "💑", 飞吻: "😘", 跳跳: "💃",
  发抖: "🥶", 怄火: "😠", 转圈: "💫", 磕头: "🙇", 挥手: "👋", 激动: "🤩", 街舞: "🕺", 献吻: "😘",
  合十: "🙏", 庆祝: "🎉", 烟花: "🎆", 爆竹: "🧨", 红包: "🧧", 發: "🀄", 福: "🧧", 鞭炮: "🧨",
  流淚: "😢", 破涕為笑: "😂", 難受: "😖", 尷尬: "😅", 難過: "🙁",
  Smile: "🙂", Grin: "😁", Joyful: "😄", Shy: "😊", Sob: "😭", Awkward: "😅", Angry: "😡", Tongue: "😜",
  Surprise: "😮", Frown: "🙁", Cool: "😎", Blush: "😊", Sleep: "😴", Drowsy: "😪", Facepalm: "🤦",
  Smirk: "😏", Smart: "🤓", Concerned: "😟", "Yeah!": "✌️", Packet: "🧧", Rose: "🌹", Heart: "❤️",
  ThumbsUp: "👍", Party: "🎉", Hug: "🤗", Shake: "🤝", Peace: "✌️", Fight: "👊", Doge: "🐶",
  Bye: "👋", Wow: "🤩", Sigh: "😮‍💨", Lol: "😂", Hurt: "🤕",
};

const CODE = /\[([^[\]\s]{1,8})\]/g;

/** Replaces known `[名称]` codes in plain text, e.g. for titles or AI prompts. */
export function emojiText(text: string): string {
  return text.replace(CODE, (code, name: string) => WECHAT_EMOJI[name] ?? code);
}

/** Splits text into strings and emoji spans; unknown codes stay as written. */
export function withEmoji(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const match of text.matchAll(CODE)) {
    const emoji = WECHAT_EMOJI[match[1]];
    if (!emoji) continue;
    if (match.index > last) parts.push(text.slice(last, match.index));
    parts.push(createElement("span", { key: `${match.index}`, className: "wx-emoji", role: "img", "aria-label": match[1], title: match[0] }, emoji));
    last = match.index + match[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}
