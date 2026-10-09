import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { mediaUrl } from "../media";
import type { MediaInfo, MessageItem } from "../types";
import { errorMessage, formatDateTime } from "../utils";
import { Icon } from "./Icons";

interface LightboxProps {
  accountId: string;
  chatId: string;
  messages: MessageItem[];
  index: number;
  onIndexChange: (index: number) => void;
  onClose: () => void;
}

export function Lightbox({ accountId, chatId, messages, index, onIndexChange, onClose }: LightboxProps) {
  const message = messages[index];
  const [info, setInfo] = useState<MediaInfo>();
  const [zoomed, setZoomed] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const hasPrevious = index > 0;
  const hasNext = index < messages.length - 1;

  useEffect(() => { closeRef.current?.focus(); }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
      else if (event.key === "ArrowLeft" && hasPrevious) onIndexChange(index - 1);
      else if (event.key === "ArrowRight" && hasNext) onIndexChange(index + 1);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [index, hasPrevious, hasNext, onIndexChange, onClose]);

  useEffect(() => {
    if (!message) return;
    let active = true;
    setInfo(undefined);
    setZoomed(false);
    api.mediaInfo(accountId, chatId, message.localId)
      .then((value) => active && setInfo(value))
      .catch((reason) => active && setInfo({ image: false, video: false, poster: false, reason: errorMessage(reason) }));
    return () => { active = false; };
  }, [accountId, chatId, message]);

  if (!message) return null;
  const url = (variant: "image" | "video" | "poster") => mediaUrl(variant, accountId, chatId, message.localId);

  let stage;
  if (!info) stage = <div className="spinner" role="status" aria-label="正在加载" />;
  else if (info.video) stage = <video key={message.localId} src={url("video")} poster={info.poster ? url("poster") : undefined} controls autoPlay />;
  else if (info.image) stage = <img key={message.localId} src={url("image")} alt={`${message.senderName || "对方"}发送的图片`} className={zoomed ? "zoomed" : ""} onClick={() => setZoomed((value) => !value)} />;
  else if (info.poster) stage = <figure><img src={url("poster")} alt="视频封面" /><figcaption>视频还没有下载到本机，请先在微信中点开它</figcaption></figure>;
  else stage = <p className="lightbox-empty">{info.reason ?? "媒体已不在本机"}</p>;

  return (
    <div className="lightbox" role="dialog" aria-modal="true" aria-label="媒体查看器" onClick={onClose}>
      <header onClick={(event) => event.stopPropagation()}>
        <div><strong>{message.isFromMe ? "我" : message.senderName || "对方"}</strong><span>{formatDateTime(message.occurredAt)} · {index + 1} / {messages.length}</span></div>
        <button ref={closeRef} type="button" className="lightbox-close" aria-label="关闭" onClick={onClose}><Icon name="close" /></button>
      </header>
      <div className={`lightbox-stage${zoomed ? " zoomed" : ""}`} onClick={(event) => event.stopPropagation()}>{stage}</div>
      {hasPrevious && <button type="button" className="lightbox-nav previous" aria-label="上一个" onClick={(event) => { event.stopPropagation(); onIndexChange(index - 1); }}><Icon name="chevronLeft" /></button>}
      {hasNext && <button type="button" className="lightbox-nav next" aria-label="下一个" onClick={(event) => { event.stopPropagation(); onIndexChange(index + 1); }}><Icon name="chevron" /></button>}
    </div>
  );
}
