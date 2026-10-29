import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icons";

interface DateFieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  className?: string;
}

const WEEKDAYS = ["日", "一", "二", "三", "四", "五", "六"];

function parseIso(value: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return undefined;
  return { year: Number(match[1]), month: Number(match[2]) - 1, day: Number(match[3]) };
}

function toIso(year: number, month: number, day: number) {
  return `${year}-${String(month + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

export function DateField({ label, value, onChange, className }: DateFieldProps) {
  const selected = parseIso(value);
  const today = new Date();
  const todayIso = toIso(today.getFullYear(), today.getMonth(), today.getDate());
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(() => ({ year: (selected ?? { year: today.getFullYear(), month: today.getMonth() }).year, month: (selected ?? { year: today.getFullYear(), month: today.getMonth() }).month }));
  const root = useRef<HTMLDivElement>(null);
  const popover = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const labelId = useId();
  const [position, setPosition] = useState({ top: 0, left: 0 });

  useEffect(() => {
    if (!open) return;
    function place() {
      const rect = button.current?.getBoundingClientRect();
      if (!rect) return;
      const width = 280;
      const left = Math.min(rect.left, Math.max(8, window.innerWidth - width - 8));
      setPosition({ top: rect.bottom + 6, left });
    }
    function onPointer(event: MouseEvent) {
      const target = event.target as Node;
      if (root.current?.contains(target) || popover.current?.contains(target)) return;
      setOpen(false);
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    place();
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  function show(next: { year: number; month: number }) {
    const month = (next.month + 12) % 12;
    const year = next.year + Math.floor(next.month / 12);
    setCursor({ year, month });
  }

  function choose(day: number) {
    onChange(toIso(cursor.year, cursor.month, day));
    setOpen(false);
  }

  const firstWeekday = new Date(cursor.year, cursor.month, 1).getDay();
  const days = new Date(cursor.year, cursor.month + 1, 0).getDate();

  return (
    <div className={`date-field${className ? ` ${className}` : ""}`} ref={root}>
      <span id={labelId}>{label}</span>
      <button
        ref={button}
        type="button"
        aria-labelledby={labelId}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          const current = selected ?? { year: today.getFullYear(), month: today.getMonth() };
          setCursor({ year: current.year, month: current.month });
          setOpen((value) => !value);
        }}
      >
        <Icon name="calendar" />
        <b>{value || "选择日期"}</b>
      </button>
      {open && createPortal(
        <div ref={popover} className="date-popover" role="dialog" aria-label={label} style={{ top: position.top, left: position.left }}>
          <div className="date-popover-head">
            <button type="button" aria-label="上个月" onClick={() => show({ year: cursor.year, month: cursor.month - 1 })}><Icon name="chevronLeft" /></button>
            <strong>{cursor.year}年{cursor.month + 1}月</strong>
            <button type="button" aria-label="下个月" onClick={() => show({ year: cursor.year, month: cursor.month + 1 })}><Icon name="chevron" /></button>
          </div>
          <div className="date-weekdays">{WEEKDAYS.map((day) => <span key={day}>{day}</span>)}</div>
          <div className="date-grid">
            {Array.from({ length: firstWeekday }, (_, index) => <span key={`blank-${index}`} />)}
            {Array.from({ length: days }, (_, index) => {
              const day = index + 1;
              const iso = toIso(cursor.year, cursor.month, day);
              return (
                <button key={iso} type="button" className={`${iso === value ? "selected" : ""}${iso === todayIso ? " today" : ""}`} aria-pressed={iso === value} onClick={() => choose(day)}>
                  {day}
                </button>
              );
            })}
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}
