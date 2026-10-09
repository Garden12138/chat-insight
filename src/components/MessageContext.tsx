import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { MessageItem } from "../types";
import { errorMessage, formatDateTime, messageKindLabel } from "../utils";
import { withEmoji } from "../emoji";
import { Icon } from "./Icons";
import { LoadingState } from "./States";

interface MessageContextProps {
  accountId: string;
  chatId: string;
  chatName: string;
  localId: number;
  /** Last message of the highlighted span; defaults to `localId`. */
  endLocalId?: number;
  onOpenDetail: (chatId: string) => void;
  onClose: () => void;
}

export function MessageContext({ accountId, chatId, chatName, localId, endLocalId = localId, onOpenDetail, onClose }: MessageContextProps) {
  const [messages, setMessages] = useState<MessageItem[]>();
  const [error, setError] = useState("");
  const closeRef = useRef<HTMLButtonElement>(null);
  const anchorRef = useRef<HTMLElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    let active = true;
    api.messageContext(accountId, chatId, localId, 8 + Math.min(20, Math.max(0, endLocalId - localId)))
      .then((value) => active && setMessages(value))
      .catch((reason) => active && setError(errorMessage(reason)));
    return () => { active = false; };
  }, [accountId, chatId, localId, endLocalId]);

  useEffect(() => {
    anchorRef.current?.scrollIntoView?.({ block: "center" });
  }, [messages]);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal context-modal" role="dialog" aria-modal="true" aria-label={`${chatName} 的聊天上下文`} onClick={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div><p className="eyebrow">CONTEXT</p><h2>{chatName}</h2></div>
          <button ref={closeRef} type="button" className="icon-button" aria-label="关闭" onClick={onClose}><Icon name="close" /></button>
        </header>
        {error ? <p className="list-empty">{error}</p> : !messages ? <LoadingState label="正在读取上下文…" /> : (
          <div className="message-timeline context-timeline">
            {messages.map((message) => {
              const highlighted = message.localId >= localId && message.localId <= endLocalId;
              return (
                <article key={message.localId} ref={message.localId === localId ? anchorRef : undefined} className={`${message.isFromMe ? "mine" : "theirs"}${highlighted ? " anchor" : ""}`}>
                  <div className="message-meta"><strong>{message.isFromMe ? "我" : message.senderName || "对方"}</strong><time>{formatDateTime(message.occurredAt)}</time></div>
                  <p>{message.text ? withEmoji(message.text) : `[${messageKindLabel[message.kind] ?? message.kind}]`}</p>
                </article>
              );
            })}
          </div>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={() => onOpenDetail(chatId)}>在聊天详情中打开</button>
        </div>
      </div>
    </div>
  );
}
