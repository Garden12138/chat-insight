export type MediaVariant = "image" | "video" | "poster";

const isWindows = () => typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");

export function mediaUrl(variant: MediaVariant, accountId: string, chatId: string, localId: number, windows = isWindows()): string {
  const path = [variant, accountId, chatId, String(localId)].map(encodeURIComponent).join("/");
  return windows ? `http://chatmedia.localhost/${path}` : `chatmedia://localhost/${path}`;
}

export const isMediaKind = (kind: string) => kind === "image" || kind === "video";
