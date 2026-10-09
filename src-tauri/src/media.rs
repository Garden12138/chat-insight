use crate::bridge::{self, BridgePaths};
use crate::error::{AppError, AppResult};
use crate::models::MediaInfo;
use crate::wechat_account::{self, AccountIdentity};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager};

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v"];
/// Upper bound for one ranged response so long videos stream instead of loading whole.
const MAX_CHUNK: u64 = 4 * 1024 * 1024;
const NOT_CURRENT_ACCOUNT: &str = "只有当前连接的微信账号可以查看图片和视频";
const MEDIA_MISSING: &str = "媒体已不在本机，可在微信中打开后重试";
const STICKER_MISSING: &str = "这个表情没有可下载的图片";
const STICKER_DOWNLOAD_FAILED: &str = "表情下载失败，请检查网络后重试";
const MAX_STICKER_BYTES: u64 = 10 * 1024 * 1024;
const STICKER_HOSTS: &[&str] = &["qq.com", "qpic.cn"];

type CacheKey = (String, String, i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Media,
    Sticker,
}

impl MediaKind {
    pub fn from_message_kind(kind: &str) -> Self {
        if kind == "sticker" {
            Self::Sticker
        } else {
            Self::Media
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickerSource {
    pub md5: String,
    pub url: String,
}

/// Resolves message media through the bridge, one bridge process at a time.
/// Stickers only exist on WeChat's CDN, so they are downloaded once into `sticker_dir`.
#[derive(Clone)]
pub struct MediaResolver {
    cache: Arc<Mutex<HashMap<CacheKey, MediaPaths>>>,
    bridge: Arc<Mutex<()>>,
    sticker_dir: PathBuf,
}

impl MediaResolver {
    pub fn new(sticker_dir: PathBuf) -> Self {
        Self {
            cache: Arc::default(),
            bridge: Arc::default(),
            sticker_dir,
        }
    }

    fn run_bridge(&self, paths: &BridgePaths, args: &[&str]) -> AppResult<Value> {
        let _guard = self
            .bridge
            .lock()
            .map_err(|_| AppError::Bridge("媒体读取任务意外结束".into()))?;
        bridge::run_json(paths, args)
    }

    pub fn resolve(
        &self,
        app: &AppHandle,
        account_id: &str,
        chat_id: &str,
        local_id: i64,
        kind: MediaKind,
    ) -> AppResult<MediaPaths> {
        let key = (account_id.to_string(), chat_id.to_string(), local_id);
        if let Some(hit) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned())
        {
            return Ok(hit);
        }
        if chat_id.starts_with('-') {
            return Err(AppError::Validation("无效的聊天标识".into()));
        }
        let identity =
            ensure_current_account(wechat_account::current_bridge_identity()?, account_id)?;
        let paths = BridgePaths::resolve(app);
        let local_id_arg = local_id.to_string();
        let found = match kind {
            MediaKind::Media => {
                let value =
                    self.run_bridge(&paths, &["media", chat_id, "--local-id", &local_id_arg])?;
                parse_media(&value, &media_roots(&identity))
            }
            MediaKind::Sticker => {
                let value = self.run_bridge(
                    &paths,
                    &[
                        "context",
                        chat_id,
                        "--local-id",
                        &local_id_arg,
                        "--before-count",
                        "0",
                        "--after-count",
                        "0",
                        "--include-debug",
                    ],
                )?;
                match parse_sticker(&value, local_id) {
                    Some(source) => {
                        let file = self.sticker_file(&source)?;
                        MediaPaths {
                            image: Some(file.clone()),
                            video: None,
                            poster: Some(file),
                        }
                    }
                    None => MediaPaths::default(),
                }
            }
        };
        if !found.is_empty() {
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(key, found.clone());
            }
        }
        Ok(found)
    }

    /// Returns the cached sticker, downloading it from WeChat's CDN on first use.
    fn sticker_file(&self, source: &StickerSource) -> AppResult<PathBuf> {
        for extension in ["gif", "png", "jpg", "webp"] {
            let cached = self.sticker_dir.join(format!("{}.{extension}", source.md5));
            if cached.is_file() {
                return Ok(cached);
            }
        }
        let failed = || AppError::Bridge(STICKER_DOWNLOAD_FAILED.into());
        let response = ureq::get(&source.url)
            .timeout(std::time::Duration::from_secs(15))
            .call()
            .map_err(|_| failed())?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_STICKER_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|_| failed())?;
        let extension = sniff_image(&bytes).ok_or_else(failed)?;
        std::fs::create_dir_all(&self.sticker_dir)?;
        let target = self.sticker_dir.join(format!("{}.{extension}", source.md5));
        let partial = target.with_extension("part");
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, &target)?;
        Ok(target)
    }

    pub fn info(
        &self,
        app: &AppHandle,
        account_id: &str,
        chat_id: &str,
        local_id: i64,
        kind: MediaKind,
    ) -> MediaInfo {
        match self.resolve(app, account_id, chat_id, local_id, kind) {
            Ok(paths) => MediaInfo {
                image: paths.image.is_some(),
                video: paths.video.is_some(),
                poster: paths.poster.is_some(),
                reason: paths.is_empty().then(|| {
                    match kind {
                        MediaKind::Media => MEDIA_MISSING,
                        MediaKind::Sticker => STICKER_MISSING,
                    }
                    .into()
                }),
            },
            Err(error) => MediaInfo {
                image: false,
                video: false,
                poster: false,
                reason: Some(error.to_string()),
            },
        }
    }
}

fn ensure_current_account(
    identity: Option<AccountIdentity>,
    account_id: &str,
) -> AppResult<AccountIdentity> {
    identity
        .filter(|identity| identity.account_id() == account_id)
        .ok_or_else(|| AppError::Bridge(NOT_CURRENT_ACCOUNT.into()))
}

/// WeChat's account files plus the bridge's decoded-image cache.
fn media_roots(identity: &AccountIdentity) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = &identity.db_root {
        roots.push(root.parent().unwrap_or(root).to_path_buf());
    }
    if let Some(dirs) = directories::BaseDirs::new() {
        roots.push(dirs.home_dir().join(".wechat-cli").join("media-cache"));
    }
    roots
}

/// Serves `chatmedia://localhost/<variant>/<account>/<chat>/<local_id>`.
pub fn respond(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    serve(app, request).unwrap_or_else(|status| {
        Response::builder()
            .status(status)
            .body(Vec::new())
            .unwrap_or_default()
    })
}

fn serve(app: &AppHandle, request: &Request<Vec<u8>>) -> Result<Response<Vec<u8>>, StatusCode> {
    let media = parse_request_path(request.uri().path()).ok_or(StatusCode::BAD_REQUEST)?;
    let kind = match media.variant {
        MediaVariant::Sticker => MediaKind::Sticker,
        _ => MediaKind::Media,
    };
    let paths = app
        .state::<MediaResolver>()
        .resolve(app, &media.account_id, &media.chat_id, media.local_id, kind)
        .map_err(|_| StatusCode::FORBIDDEN)?;
    let path = match media.variant {
        MediaVariant::Image | MediaVariant::Sticker => paths.image,
        MediaVariant::Video => paths.video,
        MediaVariant::Poster => paths.poster.or(paths.image),
    }
    .ok_or(StatusCode::NOT_FOUND)?;
    let internal = |_| StatusCode::INTERNAL_SERVER_ERROR;
    let mut file = std::fs::File::open(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    let len = file.metadata().map_err(internal)?.len();
    let builder = Response::builder()
        .header(header::CONTENT_TYPE, content_type(&path))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, max-age=3600");
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let Some(range) = range else {
        let mut body = Vec::with_capacity(len as usize);
        file.read_to_end(&mut body).map_err(internal)?;
        return builder
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, body.len())
            .body(body)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    };
    let Some((start, end)) = parse_range(range, len) else {
        return builder
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Vec::new())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    };
    let end = end.min(start + MAX_CHUNK - 1);
    let mut body = vec![0; (end - start + 1) as usize];
    file.seek(SeekFrom::Start(start)).map_err(internal)?;
    file.read_exact(&mut body).map_err(internal)?;
    builder
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
        .header(header::CONTENT_LENGTH, body.len())
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MediaPaths {
    pub image: Option<PathBuf>,
    pub video: Option<PathBuf>,
    pub poster: Option<PathBuf>,
}

impl MediaPaths {
    pub fn is_empty(&self) -> bool {
        self.image.is_none() && self.video.is_none() && self.poster.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaVariant {
    Image,
    Video,
    Poster,
    Sticker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRequest {
    pub variant: MediaVariant,
    pub account_id: String,
    pub chat_id: String,
    pub local_id: i64,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn is_image(path: &Path) -> bool {
    IMAGE_EXTENSIONS.contains(&extension(path).as_str())
}

fn is_video(path: &Path) -> bool {
    VIDEO_EXTENSIONS.contains(&extension(path).as_str())
}

/// Only existing image/video files inside one of `roots` may be served to the webview.
pub fn is_allowed(path: &Path, roots: &[PathBuf]) -> bool {
    if !(is_image(path) || is_video(path)) || !path.is_file() {
        return false;
    }
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| resolved.starts_with(root))
}

/// Picks the readable image, video and poster paths from `wechat-cli media` output.
pub fn parse_media(value: &Value, roots: &[PathBuf]) -> MediaPaths {
    let Some(item) = value
        .pointer("/data/media")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    else {
        return MediaPaths::default();
    };
    let message_kind = item.get("kind").and_then(Value::as_str).unwrap_or_default();
    let mut paths = MediaPaths::default();
    let mut consider = |path: PathBuf, family: &str| {
        if !is_allowed(&path, roots) {
            return;
        }
        if is_video(&path) {
            paths.video.get_or_insert(path);
        } else if family == "video" || message_kind == "video" {
            paths.poster.get_or_insert(path);
        } else {
            paths.image.get_or_insert(path);
        }
    };
    for (key, family) in [("images", "image"), ("videos", "video")] {
        for entry in item
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(path) = entry.get("path").and_then(Value::as_str) {
                consider(PathBuf::from(path), family);
            }
        }
    }
    for entry in item
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let family = entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(path) = entry.get("path").and_then(Value::as_str) {
            consider(PathBuf::from(path), family);
        }
    }
    if paths.poster.is_none() && message_kind == "image" {
        paths.poster = paths.image.clone();
    }
    paths
}

/// Finds the sticker's MD5 and CDN address in `wechat-cli context --include-debug` output.
pub fn parse_sticker(value: &Value, local_id: i64) -> Option<StickerSource> {
    let message = value
        .pointer("/data/messages")?
        .as_array()?
        .iter()
        .find(|item| item.pointer("/id/local_id").and_then(Value::as_i64) == Some(local_id))?;
    message
        .pointer("/debug/media_read_hints")?
        .as_array()?
        .iter()
        .filter(|hint| hint.get("resource_family").and_then(Value::as_str) == Some("sticker"))
        .find_map(|hint| {
            let md5 = hint
                .get("md5")
                .and_then(Value::as_str)?
                .to_ascii_lowercase();
            let url = hint
                .pointer("/wechat_cdn/cdn_url")
                .and_then(Value::as_str)?;
            let valid_md5 = md5.len() == 32 && md5.bytes().all(|byte| byte.is_ascii_hexdigit());
            (valid_md5 && is_sticker_url(url)).then(|| StickerSource {
                md5,
                url: url.to_string(),
            })
        })
}

/// Only WeChat's own CDN hosts may be contacted for sticker downloads.
pub fn is_sticker_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    STICKER_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Parses a single `bytes=` range into an inclusive `(start, end)` pair.
pub fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let suffix: u64 = suffix.parse().ok()?;
            if suffix == 0 {
                return None;
            }
            (len.saturating_sub(suffix), len - 1)
        }
        (start, "") => (start.parse().ok()?, len - 1),
        (start, end) => (start.parse().ok()?, end.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

pub fn content_type(path: &Path) -> &'static str {
    match extension(path).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            output.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

/// Parses `/<variant>/<account>/<chat>/<local_id>` with percent-encoded segments.
pub fn parse_request_path(path: &str) -> Option<MediaRequest> {
    let segments: Vec<_> = path.trim_start_matches('/').split('/').collect();
    let [variant, account_id, chat_id, local_id] = segments.as_slice() else {
        return None;
    };
    let variant = match *variant {
        "image" => MediaVariant::Image,
        "video" => MediaVariant::Video,
        "poster" => MediaVariant::Poster,
        "sticker" => MediaVariant::Sticker,
        _ => return None,
    };
    let account_id = percent_decode(account_id).filter(|value| !value.is_empty())?;
    let chat_id = percent_decode(chat_id).filter(|value| !value.is_empty())?;
    let local_id = percent_decode(local_id)?.parse().ok()?;
    Some(MediaRequest {
        variant,
        account_id,
        chat_id,
        local_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"x").unwrap();
        path
    }

    fn media_output(kind: &str, extra: Value) -> Value {
        let mut item = json!({ "kind": kind, "id": { "local_id": 7 } });
        item.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({ "ok": true, "data": { "media": [item] } })
    }

    #[test]
    fn parses_decoded_image() {
        let root = TempDir::new().unwrap();
        let image = touch(root.path(), "cache/a.jpg");
        let value = media_output(
            "image",
            json!({
                "images": [{ "path": image }],
                "resources": [{ "type": "image", "path": image }],
            }),
        );
        let paths = parse_media(&value, &[root.path().to_path_buf()]);
        assert_eq!(paths.image.as_deref(), Some(image.as_path()));
        assert_eq!(paths.poster.as_deref(), Some(image.as_path()));
        assert_eq!(paths.video, None);
    }

    #[test]
    fn parses_downloaded_video_and_its_thumbnail() {
        let root = TempDir::new().unwrap();
        let video = touch(root.path(), "msg/video/2026-10/abc.mp4");
        let thumb = touch(root.path(), "msg/video/2026-10/abc_thumb.jpg");
        let value = media_output(
            "video",
            json!({ "resources": [
                { "type": "video", "path": thumb },
                { "type": "video", "path": video },
            ] }),
        );
        let paths = parse_media(&value, &[root.path().to_path_buf()]);
        assert_eq!(paths.video.as_deref(), Some(video.as_path()));
        assert_eq!(paths.poster.as_deref(), Some(thumb.as_path()));
        assert_eq!(paths.image, None);
    }

    #[test]
    fn video_without_download_only_has_poster() {
        let root = TempDir::new().unwrap();
        let thumb = touch(root.path(), "msg/video/abc_thumb.jpg");
        let value = media_output(
            "video",
            json!({ "resources": [{ "type": "video", "path": thumb }] }),
        );
        let paths = parse_media(&value, &[root.path().to_path_buf()]);
        assert_eq!(paths.video, None);
        assert_eq!(paths.poster.as_deref(), Some(thumb.as_path()));
    }

    #[test]
    fn rejects_paths_outside_roots_and_unknown_types() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let foreign = touch(outside.path(), "secret.jpg");
        let raw = touch(root.path(), "img.dat");
        let escaping = root
            .path()
            .join("..")
            .join(outside.path().file_name().unwrap())
            .join("secret.jpg");
        let roots = [root.path().to_path_buf()];
        assert!(!is_allowed(&foreign, &roots));
        assert!(!is_allowed(&raw, &roots));
        assert!(!is_allowed(&escaping, &roots));
        assert!(!is_allowed(&root.path().join("missing.jpg"), &roots));
        let value = media_output("image", json!({ "images": [{ "path": foreign }] }));
        assert!(parse_media(&value, &roots).is_empty());
    }

    #[test]
    fn empty_output_has_no_media() {
        assert!(parse_media(&json!({ "ok": true, "data": { "media": [] } }), &[]).is_empty());
    }

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-5", 100), Some((95, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=20-10", 100), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }

    #[test]
    fn parses_media_request_paths() {
        assert_eq!(
            parse_request_path("/video/wechat%3Awxid_a_1234/4511%40chatroom/3032"),
            Some(MediaRequest {
                variant: MediaVariant::Video,
                account_id: "wechat:wxid_a_1234".into(),
                chat_id: "4511@chatroom".into(),
                local_id: 3032,
            })
        );
        assert_eq!(
            parse_request_path("/sticker/a/b/1").map(|request| request.variant),
            Some(MediaVariant::Sticker)
        );
        assert_eq!(parse_request_path("/file/a/b/1"), None);
        assert_eq!(parse_request_path("/image/a/b/x"), None);
        assert_eq!(parse_request_path("/image/a/b"), None);
        assert_eq!(parse_request_path("/image//b/1"), None);
    }

    #[test]
    fn only_the_connected_account_has_media() {
        let identity = AccountIdentity::from_parts(None, Some("/data/xwechat_files/wxid_a_1234"));
        assert!(ensure_current_account(identity.clone(), "wechat:wxid_a_1234").is_ok());
        let error = ensure_current_account(identity, "wechat:wxid_b_5678").unwrap_err();
        assert_eq!(error.to_string(), NOT_CURRENT_ACCOUNT);
        assert!(ensure_current_account(None, "wechat:wxid_a_1234").is_err());
    }

    #[test]
    fn media_roots_cover_account_files_and_bridge_cache() {
        let identity =
            AccountIdentity::from_parts(None, Some("/data/xwechat_files/wxid_a_1234")).unwrap();
        let roots = media_roots(&identity);
        assert_eq!(roots[0], PathBuf::from("/data/xwechat_files"));
        assert!(roots[1].ends_with(".wechat-cli/media-cache"));
    }

    fn sticker_output(hint: Value) -> Value {
        json!({ "ok": true, "data": { "messages": [
            { "id": { "local_id": 1 }, "kind": "text" },
            { "id": { "local_id": 2859 }, "kind": "sticker", "debug": { "media_read_hints": [hint] } },
        ] } })
    }

    #[test]
    fn parses_sticker_cdn_source() {
        let value = sticker_output(json!({
            "resource_family": "sticker",
            "md5": "0123456789ABCDEF0123456789abcdef",
            "wechat_cdn": { "cdn_url": "http://vweixinf.tc.qq.com/110/20401/stodownload?m=1", "encrypt_url": "http://x.qq.com/e" },
        }));
        assert_eq!(
            parse_sticker(&value, 2859),
            Some(StickerSource {
                md5: "0123456789abcdef0123456789abcdef".into(),
                url: "http://vweixinf.tc.qq.com/110/20401/stodownload?m=1".into(),
            })
        );
        assert_eq!(parse_sticker(&value, 1), None);
    }

    #[test]
    fn rejects_stickers_without_a_safe_source() {
        let no_url = sticker_output(
            json!({ "resource_family": "sticker", "md5": "0123456789abcdef0123456789abcdef" }),
        );
        assert_eq!(parse_sticker(&no_url, 2859), None);
        let bad_md5 = sticker_output(json!({
            "resource_family": "sticker", "md5": "../../etc/passwd",
            "wechat_cdn": { "cdn_url": "http://vweixinf.tc.qq.com/a" },
        }));
        assert_eq!(parse_sticker(&bad_md5, 2859), None);
        let foreign = sticker_output(json!({
            "resource_family": "sticker", "md5": "0123456789abcdef0123456789abcdef",
            "wechat_cdn": { "cdn_url": "http://evil.example.com/a" },
        }));
        assert_eq!(parse_sticker(&foreign, 2859), None);
    }

    #[test]
    fn only_wechat_cdn_hosts_are_contacted() {
        assert!(is_sticker_url("http://vweixinf.tc.qq.com/110/a"));
        assert!(is_sticker_url("https://wxapp.tc.qq.com:443/a"));
        assert!(is_sticker_url("http://mmbiz.qpic.cn/a"));
        assert!(!is_sticker_url("http://qq.com.evil.cn/a"));
        assert!(!is_sticker_url("http://evilqq.com/a"));
        assert!(!is_sticker_url("http://user@evil.cn/@qq.com"));
        assert!(!is_sticker_url("file:///etc/passwd"));
        assert!(!is_sticker_url("vweixinf.tc.qq.com/a"));
    }

    #[test]
    fn sniffs_sticker_image_formats() {
        assert_eq!(sniff_image(b"GIF89a..."), Some("gif"));
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(sniff_image(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_image(b"\x83\x06\x80)+\xc9"), None);
        assert_eq!(MediaKind::from_message_kind("sticker"), MediaKind::Sticker);
        assert_eq!(MediaKind::from_message_kind("image"), MediaKind::Media);
    }

    #[test]
    fn maps_content_types() {
        assert_eq!(content_type(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(content_type(Path::new("a.mp4")), "video/mp4");
        assert_eq!(content_type(Path::new("a.mov")), "video/quicktime");
    }
}
