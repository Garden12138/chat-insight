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
  const isVideo = message.kind === "video";
  const label = isVideo ? "视频" : "图片";

  useEffect(() => {
    if (!visible) return;
    let active = true;
    api.mediaInfo(accountId, chatId, message.localId)
      .then((value) => active && setInfo(value))
      .catch((reason) => active && setInfo({ image: false, video: false, poster: false, reason: errorMessage(reason) }));
    return () => { active = false; };
  }, [visible, accountId, chatId, message.localId]);

  if (!info) return <div ref={ref} className="media-bubble loading" role="status" aria-label={`正在加载${label}`} />;

  const viewable = info.image || info.video || info.poster;
  if (!viewable || broken) {
    return (
      <div ref={ref} className="media-bubble missing">
        <Icon name="image" />
        <span>{label}不可预览</span>
        <small>{broken ? "文件无法显示" : info.reason ?? "媒体已不在本机"}</small>
      </div>
    );
  }

  const thumb = info.poster || info.image;
  return (
    <div ref={ref} className={`media-bubble ${isVideo ? "video" : "image"}`}>
      <button type="button" onClick={onOpen} aria-label={isVideo ? "播放视频" : "查看图片"}>
        {thumb
          ? <img src={mediaUrl("poster", accountId, chatId, message.localId)} alt="" loading="lazy" onError={() => setBroken(true)} />
          : <span className="media-blank" />}
        {isVideo && <span className="media-play"><Icon name="play" /></span>}
      </button>
    </div>
  );
}
