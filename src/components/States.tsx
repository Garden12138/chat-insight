import type { ReactNode } from "react";
import { Icon } from "./Icons";

export function EmptyState({ title, description, action }: { title: string; description: string; action?: ReactNode }) {
  return (
    <div className="empty-state">
      <div className="empty-icon"><Icon name="message" /></div>
      <h3>{title}</h3>
      <p>{description}</p>
      {action}
    </div>
  );
}

export function LoadingState({ label = "正在整理本地数据…" }: { label?: string }) {
  return (
    <div className="loading-state" role="status">
      <span className="spinner" />
      <span>{label}</span>
    </div>
  );
}

export function InlineNotice({ tone = "info", children }: { tone?: "info" | "warning" | "error"; children: ReactNode }) {
  return (
    <div className={`notice ${tone}`}>
      <Icon name={tone === "info" ? "check" : "warning"} />
      <div>{children}</div>
    </div>
  );
}
