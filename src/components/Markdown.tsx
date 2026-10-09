import { Fragment, type ReactNode } from "react";
import { withEmoji } from "../emoji";

type Block =
  | { type: "heading"; text: string }
  | { type: "list"; ordered: boolean; items: string[] }
  | { type: "paragraph"; lines: string[] };

function parseMarkdown(source: string): Block[] {
  const blocks: Block[] = [];
  for (const raw of source.replace(/\r/g, "").split("\n")) {
    const line = raw.trimEnd();
    const last = blocks[blocks.length - 1];
    const heading = /^#{1,6}\s+(.*)$/.exec(line.trim());
    const bullet = /^\s*[-*•]\s+(.*)$/.exec(line);
    const numbered = /^\s*\d+[.、)]\s*(.*)$/.exec(line);
    if (!line.trim()) {
      blocks.push({ type: "paragraph", lines: [] });
    } else if (heading) {
      blocks.push({ type: "heading", text: heading[1] });
    } else if (bullet || numbered) {
      const ordered = !bullet;
      const text = (bullet ?? numbered)![1];
      if (last?.type === "list" && last.ordered === ordered) last.items.push(text);
      else blocks.push({ type: "list", ordered, items: [text] });
    } else if (last?.type === "paragraph") {
      last.lines.push(line.trim());
    } else {
      blocks.push({ type: "paragraph", lines: [line.trim()] });
    }
  }
  return blocks.filter((block) => block.type !== "paragraph" || block.lines.length > 0);
}

function inline(text: string, onCitation?: (index: number) => void): ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*|\[\d+(?:\s*[,，、]\s*\d+)*\])/g).map((part, position) => {
    if (part.startsWith("**") && part.endsWith("**") && part.length > 4) return <strong key={position}>{part.slice(2, -2)}</strong>;
    const citation = /^\[(.+)\]$/.exec(part);
    if (citation && /^\d/.test(citation[1])) {
      return (
        <span key={position} className="citations">
          {citation[1].split(/\s*[,，、]\s*/).map((value) => (
            <button key={value} type="button" className="citation" aria-label={`查看来源 ${value}`} onClick={() => onCitation?.(Number(value))}>{value}</button>
          ))}
        </span>
      );
    }
    return <Fragment key={position}>{withEmoji(part)}</Fragment>;
  });
}

export function Markdown({ text, onCitation }: { text: string; onCitation?: (index: number) => void }) {
  return (
    <div className="markdown">
      {parseMarkdown(text).map((block, index) => {
        if (block.type === "heading") return <h4 key={index}>{inline(block.text, onCitation)}</h4>;
        if (block.type === "list") {
          const items = block.items.map((item, position) => <li key={position}>{inline(item, onCitation)}</li>);
          return block.ordered ? <ol key={index}>{items}</ol> : <ul key={index}>{items}</ul>;
        }
        return <p key={index}>{block.lines.map((line, position) => <span key={position}>{position > 0 && <br />}{inline(line, onCitation)}</span>)}</p>;
      })}
    </div>
  );
}
