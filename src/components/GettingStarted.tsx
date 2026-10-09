import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { placeGuideCard, stepsFor, type GuideMode, type GuideRect } from "../guide";
import type { ViewName } from "../types";

interface GettingStartedProps {
  open: boolean;
  mode: GuideMode;
  view?: ViewName;
  onView?: (view: ViewName) => void;
  onClose: () => void;
}

function readRect(target: string): GuideRect | null {
  const element = document.querySelector<HTMLElement>(`[data-guide="${target}"]`);
  if (!element) return null;
  const box = element.getBoundingClientRect();
  if (box.width === 0 && box.height === 0) return null;
  return { top: box.top, left: box.left, width: box.width, height: box.height };
}

export function GettingStarted({ open, mode, view, onView, onClose }: GettingStartedProps) {
  const steps = stepsFor(mode);
  const [index, setIndex] = useState(0);
  const [rect, setRect] = useState<GuideRect | null>(null);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const bodyId = useId();
  const step = steps[Math.min(index, steps.length - 1)];
  const atEnd = index >= steps.length - 1;
  const primaryLabel = atEnd ? (mode === "preview" ? "知道了" : "开始使用") : "下一步";

  useEffect(() => {
    setIndex(0);
  }, [open, mode]);

  useEffect(() => {
    if (!open || !step.view || step.view === view) return;
    onView?.(step.view);
  }, [open, step, view, onView]);

  useLayoutEffect(() => {
    if (!open) return;
    if (step.view && step.view !== view) return;
    const update = () => setRect(step.target ? readRect(step.target) : null);
    update();
    const frame = requestAnimationFrame(update);
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [open, step, view]);

  useEffect(() => {
    if (!open) return;
    primaryRef.current?.focus();
  }, [open, index, mode]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const cardStyle = rect
    ? placeGuideCard(rect, { width: window.innerWidth, height: window.innerHeight })
    : undefined;

  function advance() {
    if (atEnd) onClose();
    else setIndex((current) => current + 1);
  }

  return (
    <div className={rect ? "guide-layer spotlight" : "guide-layer"}>
      <div className="guide-catcher" />
      {rect && (
        <div
          className="guide-hole"
          style={{ top: rect.top - 6, left: rect.left - 6, width: rect.width + 12, height: rect.height + 12 }}
        />
      )}
      <section
        className={rect ? "guide-card" : "guide-card centered"}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        style={cardStyle}
      >
        <p className="eyebrow">GETTING STARTED</p>
        <h2 id={titleId}>{step.title}</h2>
        <p id={bodyId}>{step.body}</p>
        <div className="guide-dots" aria-hidden="true">
          {steps.map((item, dot) => <span key={item.id} className={dot === index ? "active" : ""} />)}
        </div>
        <div className="guide-actions">
          {atEnd ? <span /> : <button type="button" className="guide-skip" onClick={onClose}>跳过</button>}
          <span className="guide-progress">第 {index + 1} 步，共 {steps.length} 步</span>
          <div className="guide-nav">
            <button type="button" className="secondary-button compact" disabled={index === 0} onClick={() => setIndex((current) => current - 1)}>上一步</button>
            <button type="button" className="primary-button compact" ref={primaryRef} onClick={advance}>{primaryLabel}</button>
          </div>
        </div>
      </section>
    </div>
  );
}
