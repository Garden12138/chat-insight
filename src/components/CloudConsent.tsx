import { useEffect, useRef } from "react";
import type { ConsentRequest } from "../ai";
import { Icon } from "./Icons";

interface CloudConsentProps {
  host: string;
  request: ConsentRequest;
  onDecide: (accepted: boolean) => void;
}

export function CloudConsent({ host, request, onDecide }: CloudConsentProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    cancelRef.current?.focus();
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onDecide(false);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDecide]);

  return (
    <div className="modal-backdrop" onClick={() => onDecide(false)}>
      <div className="modal consent-modal" role="alertdialog" aria-modal="true" aria-labelledby="consent-title" onClick={(event) => event.stopPropagation()}>
        <div className="consent-icon"><Icon name="warning" /></div>
        <p className="eyebrow">CLOUD AI</p>
        <h2 id="consent-title">{request.title}</h2>
        <p>这次请求会把以下内容发送到 <strong>{host}</strong>，由该服务处理：</p>
        <p className="consent-detail">{request.detail}</p>
        <p className="subtle">发送后的数据受该服务的隐私政策约束，Chat Insight 无法撤回。使用本机模型（如 Ollama）则不会离开这台电脑。</p>
        <div className="modal-actions">
          <button ref={cancelRef} type="button" className="secondary-button" onClick={() => onDecide(false)}>取消</button>
          <button type="button" className="primary-button" onClick={() => onDecide(true)}>确认发送</button>
        </div>
      </div>
    </div>
  );
}
