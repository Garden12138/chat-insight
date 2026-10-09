import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { mediaUrl } from "../media";
import type { MediaInfo, MessageItem } from "../types";
import { errorMessage } from "../utils";
import { Icon } from "./Icons";

interface MediaBubbleProps {
  accountId: string;
  chatId: string;
  message: MessageItem;
  onOpen: () => void;
}

const labels: Record<string, string> = { image: "图片", video: "视频", sticker: "表情" };

function useInView<T extends Element>() {
  const ref = useRef<T>(null);
  const [visible, setVisible] = useState(() => typeof IntersectionObserver === "undefined");
  useEffect(() => {
    const node = ref.current;
    if (visible || !node) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setVisible(true);
    }, { rootMargin: "240px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, [visible]);
  return [ref, visible] as const;
}

export function MediaBubble({ accountId, chatId, message, onOpen }: MediaBubbleProps) {
  const [ref, visible] = useInView<HTMLDivElement>();
  const [info, setInfo] = useState<MediaInfo>();
  const [broken, setBroken] = useState(false);
  const { kind, localId } = message;
  const isVideo = kind === "video";
  const label = labels[kind] ?? "媒体";

  useEffect(() => {
    if (!visible) return;
    let active = true;
    api.mediaInfo(accountId, chatId, localId, kind)
      .then((value) => active && setInfo(value))
      .catch((reason) => active && setInfo({ image: false, video: false, poster: false, reason: errorMessage(reason) }));
    return () => { active = false; };
  }, [visible, accountId, chatId, localId, kind]);

  if (!info) return <div ref={ref} className={`media-bubble loading${kind === "sticker" ? " sticker" : ""}`} role="status" aria-label={`正在加载${label}`} />;

  const viewable = info.image || info.video || info.poster;
  if (!viewable || broken) {
    return (
      <div ref={ref} className={`media-bubble missing${kind === "sticker" ? " sticker" : ""}`}>
        <Icon name="image" />
        <span>{label}不可预览</span>
        <small>{broken ? "文件无法显示" : info.reason ?? "媒体已不在本机"}</small>
      </div>
    );
  }

  if (kind === "sticker") {
    return (
      <div ref={ref} className="media-bubble sticker">
        <img src={mediaUrl("sticker", accountId, chatId, localId)} alt="表情" loading="lazy" onError={() => setBroken(true)} />
      </div>
    );
  }

  const thumb = info.poster || info.image;
  return (
    <div ref={ref} className={`media-bubble ${isVideo ? "video" : "image"}`}>
      <button type="button" onClick={onOpen} aria-label={isVideo ? "播放视频" : "查看图片"}>
        {thumb
          ? <img src={mediaUrl("poster", accountId, chatId, localId)} alt="" loading="lazy" onError={() => setBroken(true)} />
          : <video src={`${mediaUrl("video", accountId, chatId, localId)}#t=0.1`} preload="metadata" muted playsInline aria-hidden="true" onError={() => setBroken(true)} />}
        {isVideo && <span className="media-play"><Icon name="play" /></span>}
      </button>
    </div>
  );
}
