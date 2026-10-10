import { useEffect, useState, type ReactNode } from "react";
import { withEmoji } from "../emoji";
import { Icon } from "./Icons";

export interface MemoryAction {
  label: string;
  onClick: () => void;
  tone?: "primary" | "secondary";
}

interface MemoryCardProps {
  overdue?: boolean;
  tags: ReactNode;
  title: string;
  editing?: ReactNode;
  meta: string;
  quote: string;
  onQuote: () => void;
  hint?: ReactNode;
  primary?: MemoryAction;
  menu?: MemoryAction[];
}

export function MemoryCard({ overdue, tags, title, editing, meta, quote, onQuote, hint, primary, menu }: MemoryCardProps) {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") close(); };
    document.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <li className={`commitment-card${overdue ? " overdue" : ""}`}>
      <div className="commitment-main">
        <div className="commitment-tags">{tags}</div>
        {editing ?? <h3>{title}</h3>}
        <p className="commitment-meta">{meta}</p>
        <button type="button" className="commitment-quote" aria-label="查看原话" title="查看原话" onClick={onQuote}>
          <span>{withEmoji(quote)}</span>
          <Icon name="chevron" />
        </button>
        {hint}
      </div>
      {!editing && (primary || menu?.length) && (
        <div className="commitment-actions">
          {primary && (
            <button type="button" className={`${primary.tone === "secondary" ? "secondary" : "primary"}-button compact`} onClick={primary.onClick}>
              {primary.tone !== "secondary" && <Icon name="check" />}{primary.label}
            </button>
          )}
          {!!menu?.length && (
            <div className="more-menu">
              <button type="button" className="icon-button compact" aria-label="更多操作" aria-haspopup="menu" aria-expanded={open} onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}><Icon name="more" /></button>
              {open && (
                <div className="more-menu-list" role="menu">
                  {menu.map((action) => (
                    <button key={action.label} type="button" role="menuitem" onClick={() => { setOpen(false); action.onClick(); }}>{action.label}</button>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </li>
  );
}
