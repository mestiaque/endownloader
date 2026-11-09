//! Media sites through yt-dlp: analysis (title, thumbnail, the quality/format table) and the
//! media engine that downloads the chosen format (FFmpeg merges video + audio).
//!
//! ```text
//!  link ──is_media_url?──▶ analyze: yt-dlp -J ──▶ MediaInfo { title, thumbnail, video[], audio[] }
//!                                                         │ user picks a row
//!  queue ◀── task (engine=media, options.format = "137+140/bv*[height<=1080]+ba/b…") ◀──┘
//!    │
//!    └─▶ MediaEngine: yt-dlp -f … --progress-template ENDL… ──▶ progress / file path ──▶ events
//! ```
//! yt-dlp is always run with an argument array (no shell) and `--` before the link.
//! DRM-protected media is detected and reported as unsupported; nothing tries to get around it.

use crate::engine::{remove_quietly, Engine, EngineEvent, EngineRef, Progress, TaskSpec, Throttle};
use crate::error::DlError;
use crate::task::EngineKind;
use crate::tools::{no_window, today_days, yt_dlp_age_days, ToolKind, Tools};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

// ---------- which links are media ----------

const MEDIA_HOSTS: &[&str] = &[
    "youtube.com", "youtu.be", "youtube-nocookie.com", "tiktok.com", "instagram.com", "facebook.com", "fb.watch", "x.com", "twitter.com", "vimeo.com",
    "dailymotion.com", "dai.ly", "soundcloud.com", "twitch.tv", "reddit.com", "redd.it", "pinterest.com", "pin.it", "bilibili.com", "b23.tv",
    "bandcamp.com", "threads.net", "threads.com", "tumblr.com", "streamable.com", "rumble.com", "ok.ru", "vk.com", "vkvideo.ru", "likee.video",
    "snapchat.com", "9gag.com", "imgur.com", "kick.com", "mixcloud.com", "ted.com", "bitchute.com", "odysee.com", "nicovideo.jp", "weibo.com",
    "douyin.com", "kuaishou.com", "linkedin.com", "loom.com", "vidio.com", "bsky.app", "pornhub.com", "xvideos.com", "archive.org", "peertube.tv",
];

/// A link to a page on a media site (analysed with yt-dlp) rather than a file.
pub fn is_media_url(link: &str) -> bool {
    let Ok(u) = url::Url::parse(link) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = u.host_str().map(|h| h.to_ascii_lowercase()) else { return false };
    // archive.org also serves plain files: only its /details/ pages are media pages.
    if host.ends_with("archive.org") && !u.path().starts_with("/details/") {
        return false;
    }
    MEDIA_HOSTS.iter().any(|m| host == *m || host.ends_with(&format!(".{m}")))
}

/// A link that points at a web page rather than a file: no file extension in the last path
/// segment (or a page extension such as .html/.php). yt-dlp supports far more sites than
/// [`MEDIA_HOSTS`], so such links are analysed first instead of saving the page's HTML; when
/// yt-dlp finds nothing, the analysis card offers to download the link as a plain file.
pub fn looks_like_page(link: &str) -> bool {
    let Ok(u) = url::Url::parse(link) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let last = u.path_segments().and_then(|mut s| s.rfind(|seg| !seg.is_empty())).unwrap_or("");
    match last.rsplit_once('.') {
        None => true,
        Some((_, ext)) => {
            let ext = ext.to_ascii_lowercase();
            const PAGE: &[&str] = &["html", "htm", "shtml", "xhtml", "php", "asp", "aspx", "jsp", "cfm", "cgi", "pl"];
            // "v1.2-beta" style segments are not file extensions either.
            PAGE.contains(&ext.as_str()) || ext.len() > 5 || !ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
    }
}

/// The playlist id of a YouTube watch link that also names a playlist (`&list=…`).
pub fn youtube_list_id(link: &str) -> Option<String> {
    let u = url::Url::parse(link).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    if !(host.ends_with("youtube.com") || host == "youtu.be") {
        return None;
    }
    u.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.into_owned()).filter(|v| !v.is_empty())
}

// ---------- analysis result ----------

/// One downloadable choice (a row in the Video or Audio table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaOption {
    /// "video" | "audio"
    pub kind: String,
    /// "Best (1080p)", "720p60", "Best", "MP3"
    pub label: String,
    /// Container shown to the user: "MP4", "M4A", "MP3"…
    pub ext: String,
    pub resolution: Option<String>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    /// "avc1 + mp4a"
    pub codecs: Option<String>,
    /// Bytes; `size_estimated` when computed from bitrate × duration.
    pub size: Option<u64>,
    pub size_estimated: bool,
    /// yt-dlp format selector.
    pub format: String,
    /// `--merge-output-format`, e.g. "mp4/mkv".
    pub merge: Option<String>,
    /// `--extract-audio --audio-format <x>`.
    pub extract_audio: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// The link that was analysed.
    pub url: String,
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    /// "YouTube", "TikTok"…
    pub platform: String,
    pub is_live: bool,
    pub description: Option<String>,
    pub video: Vec<MediaOption>,
    pub audio: Vec<MediaOption>,
    /// Set for playlists, channels and profiles.
    pub entries: Vec<PlaylistEntry>,
    pub is_playlist: bool,
    /// A single video link that also names a playlist (YouTube `&list=`).
    pub playlist_id: Option<String>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string).filter(|x| !x.is_empty())
}
fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}
fn u(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(Value::as_u64).filter(|n| *n > 0)
}

fn codec_short(c: &str) -> String {
    let c = c.split('.').next().unwrap_or(c);
    match c {
        "vp09" => "vp9".into(),
        other => other.to_string(),
    }
}

fn has_codec(c: Option<&str>) -> bool {
    matches!(c, Some(x) if x != "none" && !x.is_empty())
}

/// Size of one format: exact, approximate, or bitrate × duration.
fn format_size(fmt: &Value, duration: Option<f64>) -> (Option<u64>, bool) {
    if let Some(n) = u(fmt, "filesize") {
        return (Some(n), false);
    }
    if let Some(n) = u(fmt, "filesize_approx") {
        return (Some(n), true);
    }
    match (f(fmt, "tbr"), duration) {
        (Some(tbr), Some(d)) if tbr > 0.0 && d > 0.0 => (Some((tbr * 1000.0 / 8.0 * d) as u64), true),
        _ => (None, true),
    }
}

/// Lower is better: H.264 plays everywhere, then VP9, then AV1.
fn codec_rank(vcodec: &str) -> u8 {
    match codec_short(vcodec).as_str() {
        "avc1" | "h264" => 0,
        "vp9" => 1,
        "av01" => 2,
        _ => 3,
    }
}

fn usable(fmt: &Value) -> bool {
    s(fmt, "protocol").as_deref() != Some("mhtml") && s(fmt, "format_note").as_deref() != Some("storyboard") && fmt.get("has_drm").and_then(Value::as_bool) != Some(true)
}

fn platform_name(v: &Value) -> String {
    let key = s(v, "extractor_key").or_else(|| s(v, "ie_key")).unwrap_or_else(|| "Web".into());
    // "YoutubeTab" → "YouTube", "TikTok" stays.
    let base = key.trim_end_matches("Tab").trim_end_matches("Playlist").trim_end_matches("User");
    match base.to_ascii_lowercase().as_str() {
        "youtube" => "YouTube".into(),
        "tiktok" => "TikTok".into(),
        "facebook" => "Facebook".into(),
        "instagram" => "Instagram".into(),
        "twitter" => "X / Twitter".into(),
        "generic" => "Web".into(),
        _ => base.to_string(),
    }
}

fn best_thumbnail(v: &Value) -> Option<String> {
    s(v, "thumbnail").or_else(|| {
        v.get("thumbnails")?.as_array()?.iter().rev().find_map(|t| s(t, "url"))
    })
}

fn generic_options(duration: Option<f64>) -> (Vec<MediaOption>, Vec<MediaOption>) {
    let mut video = vec![MediaOption {
        kind: "video".into(),
        label: "Best".into(),
        ext: "MP4".into(),
        resolution: None,
        height: None,
        fps: None,
        codecs: None,
        size: None,
        size_estimated: true,
        format: "bv*+ba/b".into(),
        merge: Some("mp4/mkv".into()),
        extract_audio: None,
    }];
    for h in [2160u32, 1440, 1080, 720, 480, 360] {
        video.push(MediaOption { label: format!("{h}p"), height: Some(h), format: height_selector(h), ..video[0].clone() });
    }
    (video, audio_options(None, duration))
}

/// Up to `h` pixels high (playlists, the command line and fallbacks): H.264 + M4A first so
/// the result is an MP4 that plays everywhere, else the best available.
pub fn height_selector(h: u32) -> String {
    format!("bv*[height<={h}][vcodec^=avc1]+ba[ext=m4a]/bv*[height<={h}]+ba/b[height<={h}]")
}

/// The download choice for a quality setting ("best", "1080", "720p", "audio", "mp3"),
/// used where no analysis card is shown (command line, playlists).
pub fn quality_option(q: &str) -> MediaOption {
    let q = q.trim().trim_end_matches(['p', 'P']).to_ascii_lowercase();
    let base = MediaOption {
        kind: "video".into(),
        label: "Best".into(),
        ext: "MP4".into(),
        resolution: None,
        height: None,
        fps: None,
        codecs: None,
        size: None,
        size_estimated: true,
        format: "bv*+ba/b".into(),
        merge: Some("mp4/mkv".into()),
        extract_audio: None,
    };
    match q.as_str() {
        "audio" | "m4a" => MediaOption { kind: "audio".into(), label: "Audio".into(), ext: "M4A".into(), format: "ba[ext=m4a]/ba/b".into(), merge: None, ..base },
        "mp3" => MediaOption { kind: "audio".into(), label: "MP3".into(), ext: "MP3".into(), format: "ba/b".into(), merge: None, extract_audio: Some("mp3".into()), ..base },
        n => match n.parse::<u32>() {
            Ok(h) if (100..=4320).contains(&h) => MediaOption { label: format!("{h}p"), height: Some(h), format: height_selector(h), ..base },
            _ => base,
        },
    }
}

fn audio_options(best: Option<&Value>, duration: Option<f64>) -> Vec<MediaOption> {
    let (size, est) = best.map(|b| format_size(b, duration)).unwrap_or((None, true));
    let ext = best.and_then(|b| s(b, "ext")).unwrap_or_else(|| "m4a".into());
    let codec = best.and_then(|b| s(b, "acodec")).map(|c| codec_short(&c));
    vec![
        MediaOption {
            kind: "audio".into(),
            label: "Best".into(),
            ext: ext.to_ascii_uppercase(),
            resolution: None,
            height: None,
            fps: None,
            codecs: codec,
            size,
            size_estimated: est,
            format: "ba[ext=m4a]/ba/b".into(),
            merge: None,
            extract_audio: None,
        },
        MediaOption {
            kind: "audio".into(),
            label: "MP3".into(),
            ext: "MP3".into(),
            resolution: None,
            height: None,
            fps: None,
            codecs: Some("mp3".into()),
            // 192 kbit/s VBR on average.
            size: duration.map(|d| (d * 192_000.0 / 8.0) as u64),
            size_estimated: true,
            format: "ba/b".into(),
            merge: None,
            extract_audio: Some("mp3".into()),
        },
    ]
}

/// Build the quality tables from `yt-dlp -J` output.
pub fn parse_info(v: &Value, requested_url: &str) -> Result<MediaInfo, DlError> {
    let duration = f(v, "duration");
    let is_playlist = matches!(s(v, "_type").as_deref(), Some("playlist" | "multi_video"));
    let mut info = MediaInfo {
        url: requested_url.to_string(),
        id: s(v, "id").unwrap_or_default(),
        title: s(v, "title").or_else(|| s(v, "fulltitle")).unwrap_or_else(|| "Untitled".into()),
        thumbnail: best_thumbnail(v),
        duration,
        uploader: s(v, "uploader").or_else(|| s(v, "channel")).or_else(|| s(v, "uploader_id")),
        platform: platform_name(v),
        is_live: v.get("is_live").and_then(Value::as_bool).unwrap_or(false),
        description: s(v, "description").map(|d| d.chars().take(2000).collect()),
        video: Vec::new(),
        audio: Vec::new(),
        entries: Vec::new(),
        is_playlist,
        playlist_id: youtube_list_id(requested_url).filter(|_| !is_playlist),
    };

    if is_playlist {
        let entries = v.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
        info.entries = entries
            .iter()
            .filter_map(|e| {
                let url = s(e, "webpage_url").or_else(|| s(e, "url"))?;
                let id = s(e, "id").unwrap_or_else(|| url.clone());
                Some(PlaylistEntry { id, title: s(e, "title").unwrap_or_else(|| url.clone()), url, duration: f(e, "duration"), thumbnail: best_thumbnail(e) })
            })
            .collect();
        if info.thumbnail.is_none() {
            info.thumbnail = info.entries.iter().find_map(|e| e.thumbnail.clone());
        }
        let (video, audio) = generic_options(None);
        info.video = video;
        info.audio = audio;
        return Ok(info);
    }

    let all: Vec<Value> = match v.get("formats").and_then(Value::as_array) {
        Some(list) => list.clone(),
        None => vec![v.clone()], // single-format result (direct media)
    };
    let drm_only = !all.is_empty() && all.iter().all(|x| x.get("has_drm").and_then(Value::as_bool) == Some(true) || !usable(x));
    if drm_only && all.iter().any(|x| x.get("has_drm").and_then(Value::as_bool) == Some(true)) {
        return Err(DlError::Drm);
    }
    let formats: Vec<&Value> = all.iter().filter(|x| usable(x)).collect();

    // Best audio-only stream (M4A preferred: it fits MP4 without re-encoding).
    let audio_only: Vec<&Value> = formats.iter().copied().filter(|x| !has_codec(x.get("vcodec").and_then(Value::as_str)) && has_codec(x.get("acodec").and_then(Value::as_str))).collect();
    let abr = |x: &Value| f(x, "abr").or_else(|| f(x, "tbr")).unwrap_or(0.0);
    let best_audio = audio_only
        .iter()
        .copied()
        .filter(|x| s(x, "ext").as_deref() == Some("m4a"))
        .max_by(|a, b| abr(a).total_cmp(&abr(b)))
        .or_else(|| audio_only.iter().copied().max_by(|a, b| abr(a).total_cmp(&abr(b))));

    // One row per height: the most compatible codec, direct HTTP before HLS, then bitrate.
    let mut by_height: HashMap<u32, Vec<&Value>> = HashMap::new();
    for x in formats.iter().copied() {
        if !has_codec(x.get("vcodec").and_then(Value::as_str)) {
            continue;
        }
        if let Some(h) = u(x, "height") {
            by_height.entry(h as u32).or_default().push(x);
        }
    }
    let mut heights: Vec<u32> = by_height.keys().copied().collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    for h in heights {
        let Some(cands) = by_height.get(&h) else { continue };
        let pick = cands.iter().copied().min_by(|a, b| {
            let key = |x: &Value| {
                let http = s(x, "protocol").is_some_and(|p| p.starts_with("http"));
                (codec_rank(x.get("vcodec").and_then(Value::as_str).unwrap_or("")), !http)
            };
            key(a).cmp(&key(b)).then(f(b, "tbr").unwrap_or(0.0).total_cmp(&f(a, "tbr").unwrap_or(0.0)))
        });
        let Some(p) = pick else { continue };
        let vid = s(p, "format_id").unwrap_or_default();
        let vcodec = codec_short(p.get("vcodec").and_then(Value::as_str).unwrap_or(""));
        let combined = has_codec(p.get("acodec").and_then(Value::as_str));
        let (vsize, vest) = format_size(p, duration);
        let fps = f(p, "fps").map(|x| x.round());
        let label = match fps {
            Some(r) if r > 30.0 => format!("{h}p{r}"),
            _ => format!("{h}p"),
        };
        let (format, size, est, codecs) = if combined || best_audio.is_none() {
            let ac = p.get("acodec").and_then(Value::as_str).filter(|c| has_codec(Some(c))).map(codec_short);
            (format!("{vid}/{}", height_selector(h)), vsize, vest, Some(ac.map_or(vcodec.clone(), |a| format!("{vcodec} + {a}"))))
        } else {
            let a = best_audio.unwrap_or(p);
            let aid = s(a, "format_id").unwrap_or_default();
            let (asize, aest) = format_size(a, duration);
            let acodec = codec_short(a.get("acodec").and_then(Value::as_str).unwrap_or(""));
            (format!("{vid}+{aid}/{}", height_selector(h)), vsize.zip(asize).map(|(x, y)| x + y).or(vsize), vest || aest, Some(format!("{vcodec} + {acodec}")))
        };
        let resolution = match (u(p, "width"), u(p, "height")) {
            (Some(w), Some(hh)) => Some(format!("{w}x{hh}")),
            _ => s(p, "resolution"),
        };
        info.video.push(MediaOption {
            kind: "video".into(),
            label,
            ext: "MP4".into(),
            resolution,
            height: Some(h),
            fps,
            codecs,
            size,
            size_estimated: est,
            format,
            merge: Some("mp4/mkv".into()),
            extract_audio: None,
        });
    }
    if let Some(top) = info.video.first().cloned() {
        let best = MediaOption { label: format!("Best ({})", top.label), format: format!("{}/bv*+ba/b", top.format.split('/').next().unwrap_or("bv*+ba")), ..top };
        info.video.insert(0, best);
    }
    if best_audio.is_some() || !info.video.is_empty() {
        info.audio = audio_options(best_audio, duration);
    }
    if info.video.is_empty() && info.audio.is_empty() {
        return Err(DlError::Other("No downloadable formats were found for this link".into()));
    }
    Ok(info)
}

// ---------- errors ----------

/// The last `ERROR:` line of yt-dlp's output, without the `[extractor] id:` prefix.
pub fn last_error(stderr: &str) -> Option<String> {
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:"))?;
    let msg = line.trim_start_matches("ERROR:").trim();
    // "[youtube] abc123: Video unavailable" → "Video unavailable"
    let msg = match msg.strip_prefix('[').and_then(|r| r.split_once(']')) {
        Some((_, rest)) => rest.trim_start().split_once(": ").map_or(rest.trim(), |(_, m)| m.trim()),
        None => msg,
    };
    Some(msg.to_string())
}

/// Map a yt-dlp error message to a typed error. `yt_dlp_age` (days) turns extraction
/// failures into "yt-dlp is outdated" when the copy is old.
pub fn map_error(msg: &str, yt_dlp_age: Option<i64>) -> DlError {
    let l = msg.to_ascii_lowercase();
    let has = |p: &[&str]| p.iter().any(|x| l.contains(x));
    if has(&["drm"]) {
        DlError::Drm
    } else if has(&["sign in to confirm", "age-restricted", "login required", "log in", "logged-in", "requires authentication", "use --cookies", "cookies-from-browser", "private account", "members-only", "join this channel"]) {
        DlError::LoginRequired
    } else if has(&["available in your country", "geo restrict", "geo-restrict", "not available in your region", "blocked it in your country"]) {
        DlError::GeoBlocked
    } else if has(&["private video", "video unavailable", "video is unavailable", "has been removed", "no longer available", "account associated", "does not exist", "has been terminated", "deleted", "this post is unavailable"]) {
        DlError::Removed
    } else if has(&["http error 429", "too many requests", "rate-limit", "rate limit"]) {
        DlError::RateLimited
    } else if has(&["http error 404"]) {
        DlError::NotFound
    } else if has(&["http error 403"]) {
        DlError::Forbidden
    } else if has(&["no space left", "disk full"]) {
        DlError::DiskFull
    } else if has(&["unable to download webpage", "timed out", "connection reset", "connection refused", "name or service not known", "temporary failure in name resolution", "network is unreachable", "getaddrinfo", "read timed out", "incomplete read", "http error 5"]) {
        DlError::Network(msg.to_string())
    } else if l.contains("unsupported url") {
        // No extractor matched (or the site redirected to an age/login check): updating yt-dlp
        // would not help, so never call this "outdated".
        DlError::Other(
            "No video or audio was found on this page. It may be a category, search, sign-in or age-check page: open one video and paste its link, or use Download as a file.".into(),
        )
    } else if has(&["unable to extract", "nsig", "signature", "requested format is not available", "no video formats found", "failed to parse", "please report this issue"]) {
        // The site changed and the extractor broke: a newer yt-dlp usually fixes this.
        if yt_dlp_age.is_some_and(|d| d > 45) {
            DlError::ToolOutdated("yt-dlp".into())
        } else {
            DlError::Other(msg.to_string())
        }
    } else {
        DlError::Other(msg.to_string())
    }
}

// ---------- running yt-dlp ----------

/// Per-user media preferences the engine reads when it starts yt-dlp.
#[derive(Debug, Clone, Default)]
pub struct MediaPrefs {
    /// `--cookies-from-browser` value ("firefox", "chrome"…), empty = none.
    pub cookies_browser: String,
}

pub type PrefsSource = Arc<dyn Fn() -> MediaPrefs + Send + Sync>;

fn common_args(tools: &Tools, prefs: &MediaPrefs) -> Vec<String> {
    let mut a: Vec<String> = vec!["--ignore-config".into(), "--no-colors".into(), "--encoding".into(), "utf-8".into()];
    a.extend(tools.js_runtime_args());
    if !prefs.cookies_browser.trim().is_empty() {
        a.push("--cookies-from-browser".into());
        a.push(prefs.cookies_browser.trim().to_string());
    }
    a
}

fn yt_dlp_age(tools: &Tools, version: Option<&str>) -> Option<i64> {
    let _ = tools;
    version.and_then(|v| yt_dlp_age_days(v, today_days()))
}

/// Analyse a link: `yt-dlp -J`. `playlist` = list every entry of a playlist/channel
/// (flat, fast) instead of the single video.
pub async fn analyze(tools: &Tools, prefs: &MediaPrefs, link: &str, playlist: bool) -> Result<MediaInfo, DlError> {
    tools.ensure_media_tools(false).await?;
    let ytdlp = tools.path(ToolKind::YtDlp).ok_or_else(|| DlError::ToolMissing("yt-dlp".into()))?;
    let mut args = common_args(tools, prefs);
    args.extend(["-J".into(), "--no-warnings".into()]);
    if playlist {
        args.push("--flat-playlist".into());
        args.push("--yes-playlist".into());
    } else {
        args.push("--no-playlist".into());
    }
    args.push("--".into());
    args.push(link.to_string());
    let mut cmd = Command::new(&ytdlp);
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    no_window(&mut cmd);
    let started = std::time::Instant::now();
    let out = tokio::time::timeout(Duration::from_secs(120), cmd.output())
        .await
        .map_err(|_| DlError::Network("the site took too long to answer (2 minutes)".into()))?
        .map_err(|e| DlError::ToolMissing(format!("yt-dlp ({e})")))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let msg = last_error(&stderr).unwrap_or_else(|| stderr.lines().last().unwrap_or("yt-dlp failed").to_string());
        log::warn!("analyze failed: {msg}");
        let version = tools.version(ToolKind::YtDlp).await;
        return Err(map_error(&msg, yt_dlp_age(tools, version.as_deref())));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| DlError::Other(format!("yt-dlp gave unreadable output: {e}")))?;
    let info = parse_info(&v, link)?;
    log::info!("analyzed {} link in {:.1}s: {} video / {} audio option(s), {} entr(ies)", info.platform, started.elapsed().as_secs_f64(), info.video.len(), info.audio.len(), info.entries.len());
    Ok(info)
}

/// Sums progress over the streams yt-dlp downloads one after another (video, then audio).
#[derive(Debug, Default)]
pub struct StreamProgress {
    streams: Vec<(String, u64, Option<u64>)>,
    expected_streams: usize,
    expected_total: Option<u64>,
}

impl StreamProgress {
    pub fn new(expected_streams: usize, expected_total: Option<u64>) -> Self {
        Self { streams: Vec::new(), expected_streams: expected_streams.max(1), expected_total }
    }

    pub fn update(&mut self, id: &str, done: u64, total: Option<u64>) {
        match self.streams.iter_mut().find(|(s, _, _)| s == id) {
            Some(e) => {
                e.1 = done;
                if total.is_some() {
                    e.2 = total;
                }
            }
            None => self.streams.push((id.to_string(), done, total)),
        }
    }

    pub fn done(&self) -> u64 {
        self.streams.iter().map(|(_, d, _)| d).sum()
    }

    /// Total for the bar: the streams' own sizes once all are known, else the estimate
    /// from the analysis (never below what is already downloaded).
    pub fn total(&self) -> Option<u64> {
        let known: u64 = self.streams.iter().filter_map(|(_, _, t)| *t).sum();
        let all_known = self.streams.len() >= self.expected_streams && self.streams.iter().all(|(_, _, t)| t.is_some());
        let t = if all_known { Some(known) } else { self.expected_total.map(|e| e.max(known)).or(if known > 0 { Some(known) } else { None }) };
        t.map(|t| t.max(self.done()))
    }
}

/// One parsed `ENDL …` progress line.
#[derive(Debug, PartialEq)]
pub struct ProgressLine {
    pub format_id: String,
    pub done: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub eta: Option<u64>,
}

/// One `ENDL <status> <done> <total> <estimate> <speed> <eta> <format_id>` line.
pub fn parse_progress_line(line: &str) -> Option<ProgressLine> {
    let rest = line.strip_prefix("ENDL ")?;
    let p: Vec<&str> = rest.split_whitespace().collect();
    if p.len() < 7 {
        return None;
    }
    let num = |x: &str| x.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as u64);
    let total = num(p[2]).or_else(|| num(p[3])).filter(|t| *t > 0);
    // A stream already on disk (resume after pause) is reported as `finished` with no
    // downloaded_bytes; count it as complete so the bar does not drop back.
    let done = num(p[1]).or(if p[0] == "finished" { total } else { None }).unwrap_or(0);
    Some(ProgressLine { format_id: p[6].to_string(), done, total, speed: num(p[4]).unwrap_or(0), eta: num(p[5]) })
}

struct Proc {
    pid: Option<u32>,
    /// Set when we stop it on purpose (pause/cancel/shutdown): no Failed event then.
    stopped: Arc<AtomicBool>,
    exited: Arc<tokio::sync::Notify>,
    done: Arc<AtomicBool>,
    folder: PathBuf,
    media_id: Option<String>,
}

pub struct MediaEngine {
    tools: Tools,
    prefs: PrefsSource,
    procs: Arc<Mutex<HashMap<String, Proc>>>,
    seq: std::sync::atomic::AtomicU64,
}

impl MediaEngine {
    pub fn new(tools: Tools, prefs: PrefsSource) -> Arc<Self> {
        Arc::new(Self { tools, prefs, procs: Arc::new(Mutex::new(HashMap::new())), seq: std::sync::atomic::AtomicU64::new(0) })
    }

    /// Stop a running yt-dlp (and the FFmpeg it started) and wait until it is gone.
    async fn stop(&self, key: &str) -> Option<(PathBuf, Option<String>)> {
        let (pid, exited, done, folder, media_id) = {
            let procs = self.procs.lock();
            let p = procs.get(key)?;
            p.stopped.store(true, Ordering::SeqCst);
            (p.pid, p.exited.clone(), p.done.clone(), p.folder.clone(), p.media_id.clone())
        };
        if let Some(pid) = pid {
            kill_tree(pid);
        }
        if !done.load(Ordering::SeqCst) {
            let _ = tokio::time::timeout(Duration::from_secs(5), exited.notified()).await;
        }
        self.procs.lock().remove(key);
        Some((folder, media_id))
    }
}

/// Terminate a process and its children (yt-dlp starts FFmpeg).
fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;
        // The child was started in its own process group (pgid = pid).
        if let Err(e) = killpg(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            log::debug!("killpg {pid}: {e}");
        }
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        let _ = cmd.output();
    }
}

/// yt-dlp's temporary files for a media id (partial streams, fragments, unmerged parts).
pub fn partial_files(folder: &Path, media_id: &str) -> Vec<PathBuf> {
    let tag = format!("[{media_id}]");
    let Ok(rd) = std::fs::read_dir(folder) else { return Vec::new() };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else { return false };
            if !name.contains(&tag) {
                return false;
            }
            let intermediate = name.rsplit('.').nth(1).is_some_and(|seg| seg.len() > 1 && seg.starts_with('f') && seg[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
            name.ends_with(".part") || name.ends_with(".ytdl") || name.contains(".part-Frag") || name.ends_with(".temp") || intermediate
        })
        .collect()
}

#[async_trait]
impl Engine for MediaEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Media
    }

    async fn start(&self, spec: TaskSpec, events: mpsc::Sender<EngineEvent>) -> Result<EngineRef, DlError> {
        // First media download on a fresh install (command line, playlist): fetch yt-dlp & co.
        self.tools.ensure_media_tools(false).await?;
        let ytdlp = self.tools.path(ToolKind::YtDlp).ok_or_else(|| DlError::ToolMissing("yt-dlp".into()))?;
        let opt = |k: &str| spec.options.get(k).and_then(Value::as_str).map(str::to_string).filter(|v| !v.is_empty());
        let format = opt("format").unwrap_or_else(|| "bv*+ba/b".into());
        let extract = opt("extractAudio");
        let needs_ffmpeg = format.contains('+') || extract.is_some();
        let ffmpeg = self.tools.path(ToolKind::Ffmpeg);
        if needs_ffmpeg && ffmpeg.is_none() {
            return Err(DlError::ToolMissing("FFmpeg".into()));
        }
        std::fs::create_dir_all(&spec.folder)?;
        let prefs = (self.prefs)();
        let mut args = common_args(&self.tools, &prefs);
        args.extend(
            [
                "--no-playlist",
                "--newline",
                "--progress",
                "--no-mtime",
                "--continue",
                "--part",
                "--retries",
                "5",
                "--fragment-retries",
                "10",
                "--concurrent-fragments",
                "4",
                "--progress-template",
                "download:ENDL %(progress.status)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s %(info.format_id)s",
                "--print",
                "after_move:ENDLFILE %(filepath)s",
            ]
            .map(String::from),
        );
        if let Some(ff) = &ffmpeg {
            args.push("--ffmpeg-location".into());
            args.push(ff.display().to_string());
        }
        args.push("-f".into());
        args.push(format.clone());
        if let Some(m) = opt("merge") {
            args.push("--merge-output-format".into());
            args.push(m);
        }
        if let Some(a) = &extract {
            args.extend(["--extract-audio".into(), "--audio-format".into(), a.clone(), "--audio-quality".into(), "0".into()]);
        }
        if spec.speed_limit > 0 {
            args.push("--limit-rate".into());
            args.push(spec.speed_limit.to_string());
        }
        if cfg!(windows) {
            args.push("--windows-filenames".into());
        }
        args.extend(["-P".into(), spec.folder.display().to_string(), "-o".into(), "%(title).150B [%(id)s].%(ext)s".into(), "--".into(), spec.url.clone()]);

        let mut cmd = Command::new(&ytdlp);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        no_window(&mut cmd);
        #[cfg(unix)]
        {
            // Own process group, so stopping it also stops the FFmpeg it starts.
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| DlError::ToolMissing(format!("yt-dlp ({e})")))?;
        let pid = child.id();
        let key = format!("m{}-{}", spec.task_id, self.seq.fetch_add(1, Ordering::Relaxed));
        let stopped = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(tokio::sync::Notify::new());
        let done = Arc::new(AtomicBool::new(false));
        let media_id = opt("mediaId");
        self.procs.lock().insert(key.clone(), Proc { pid, stopped: stopped.clone(), exited: exited.clone(), done: done.clone(), folder: spec.folder.clone(), media_id });
        log::info!("media task {} started: yt-dlp -f {format}", spec.task_id);

        let streams = format.split('/').next().map_or(1, |first| first.matches('+').count() + 1);
        let expected = spec.options.get("size").and_then(Value::as_u64);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tools = self.tools.clone();
        let procs = self.procs.clone();
        let task_id = spec.task_id;
        let key2 = key.clone();
        tokio::spawn(async move {
            // stderr: keep the tail for the error message; log warnings.
            let err_task = tokio::spawn(async move {
                let mut tail = String::new();
                if let Some(e) = stderr {
                    let mut lines = BufReader::new(e).lines();
                    while let Ok(Some(l)) = lines.next_line().await {
                        if l.starts_with("ERROR:") || l.starts_with("WARNING:") {
                            log::debug!("yt-dlp: {l}");
                        }
                        tail.push_str(&l);
                        tail.push('\n');
                        if tail.len() > 16_384 {
                            tail.drain(..tail.len() - 8192);
                        }
                    }
                }
                tail
            });
            let mut agg = StreamProgress::new(streams, expected);
            let mut throttle = Throttle::new(4);
            let mut path: Option<PathBuf> = None;
            if let Some(o) = stdout {
                let mut lines = BufReader::new(o).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if let Some(pl) = parse_progress_line(&l) {
                        agg.update(&pl.format_id, pl.done, pl.total);
                        if throttle.ready() {
                            let p = Progress { done: agg.done(), total: agg.total(), speed: pl.speed, eta_secs: pl.eta, connections: 1 };
                            let _ = events.try_send(EngineEvent::Progress(p));
                        }
                    } else if let Some(p) = l.strip_prefix("ENDLFILE ") {
                        path = Some(PathBuf::from(p.trim()));
                    }
                }
            }
            let status = child.wait().await;
            let tail = err_task.await.unwrap_or_default();
            done.store(true, Ordering::SeqCst);
            exited.notify_waiters();
            if stopped.load(Ordering::SeqCst) {
                return;
            }
            procs.lock().remove(&key2);
            let ok = status.as_ref().map(|s| s.success()).unwrap_or(false);
            match (ok, path) {
                (true, Some(p)) => {
                    let size = std::fs::metadata(&p).map(|m| m.len()).ok();
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                    let _ = events.send(EngineEvent::Meta { title: None, filename: name, total: size }).await;
                    let _ = events.send(EngineEvent::Finished { path: p }).await;
                }
                (true, None) => {
                    let _ = events.send(EngineEvent::Failed(DlError::Other("yt-dlp finished without producing a file".into()))).await;
                }
                (false, _) => {
                    let msg = last_error(&tail).unwrap_or_else(|| tail.lines().last().unwrap_or("yt-dlp failed").to_string());
                    log::warn!("media task {task_id} failed: {msg}");
                    let version = tools.version(ToolKind::YtDlp).await;
                    let _ = events.send(EngineEvent::Failed(map_error(&msg, version.as_deref().and_then(|v| yt_dlp_age_days(v, today_days()))))).await;
                }
            }
        });
        Ok(EngineRef { task_id: spec.task_id, key })
    }

    /// yt-dlp cannot pause: stop it, keeping its `.part` files; the next start continues them.
    async fn pause(&self, r: &EngineRef) -> Result<(), DlError> {
        self.stop(&r.key).await;
        Ok(())
    }

    async fn resume(&self, _r: &EngineRef) -> Result<(), DlError> {
        Err(DlError::Other("yt-dlp restarts to resume".into()))
    }

    async fn cancel(&self, r: &EngineRef, keep_partial: bool) -> Result<(), DlError> {
        if let Some((folder, Some(id))) = self.stop(&r.key).await {
            if !keep_partial {
                for p in partial_files(&folder, &id) {
                    remove_quietly(&p);
                }
            }
        }
        Ok(())
    }

    async fn shutdown(&self) {
        let keys: Vec<String> = self.procs.lock().keys().cloned().collect();
        for k in keys {
            self.stop(&k).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn media_links() {
        assert!(is_media_url("https://www.youtube.com/watch?v=mk48xRzuNvA&list=RDmk48xRzuNvA"));
        assert!(is_media_url("https://youtu.be/abc"));
        assert!(is_media_url("https://www.tiktok.com/@u/photo/123"));
        assert!(is_media_url("https://m.facebook.com/watch/?v=1"));
        assert!(!is_media_url("https://releases.ubuntu.com/24.04/ubuntu.iso"));
        assert!(!is_media_url("https://notyoutube.com/x"));
        assert!(!is_media_url("https://archive.org/download/x/file.zip"));
        assert!(is_media_url("https://archive.org/details/some-film"));
        assert_eq!(youtube_list_id("https://www.youtube.com/watch?v=a&list=RDa&start_radio=1").as_deref(), Some("RDa"));
        assert_eq!(youtube_list_id("https://www.youtube.com/watch?v=a"), None);
    }

    fn fmt(id: &str, ext: &str, h: Option<u64>, v: &str, a: &str, size: Option<u64>, proto: &str) -> Value {
        json!({"format_id": id, "ext": ext, "height": h, "width": h.map(|h| h * 16 / 9), "fps": 24, "vcodec": v, "acodec": a, "filesize": size, "protocol": proto, "tbr": 1000.0})
    }

    fn sample() -> Value {
        json!({
            "id": "mk48xRzuNvA", "title": "The Script - Hall of Fame", "duration": 233.0, "uploader": "The Script",
            "extractor_key": "Youtube", "thumbnail": "https://i.ytimg.com/vi/mk48xRzuNvA/maxresdefault.jpg",
            "formats": [
                {"format_id": "sb0", "ext": "mhtml", "protocol": "mhtml", "format_note": "storyboard", "vcodec": "none", "acodec": "none"},
                fmt("140", "m4a", None, "none", "mp4a.40.2", Some(3_773_001), "https"),
                fmt("251", "webm", None, "none", "opus", Some(3_731_668), "https"),
                fmt("137", "mp4", Some(1080), "avc1.640028", "none", Some(66_072_637), "https"),
                fmt("248", "webm", Some(1080), "vp9", "none", Some(30_067_397), "https"),
                fmt("270", "mp4", Some(1080), "avc1.640028", "none", None, "m3u8_native"),
                fmt("247", "webm", Some(720), "vp9", "none", Some(18_702_353), "https"),
                fmt("398", "mp4", Some(720), "av01.0.05M.08", "none", Some(17_953_059), "https"),
                fmt("18", "mp4", Some(360), "avc1.42001E", "mp4a.40.2", Some(17_000_000), "https"),
            ]
        })
    }

    #[test]
    fn builds_quality_table() {
        let i = parse_info(&sample(), "https://www.youtube.com/watch?v=mk48xRzuNvA&list=RDmk48xRzuNvA").unwrap();
        assert_eq!(i.title, "The Script - Hall of Fame");
        assert_eq!(i.platform, "YouTube");
        assert_eq!(i.playlist_id.as_deref(), Some("RDmk48xRzuNvA"));
        let labels: Vec<&str> = i.video.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, vec!["Best (1080p)", "1080p", "720p", "360p"]);
        // 1080p: H.264 over VP9, direct HTTP over HLS, best M4A audio added.
        let r1080 = &i.video[1];
        assert!(r1080.format.starts_with("137+140/"), "{}", r1080.format);
        assert_eq!(r1080.size, Some(66_072_637 + 3_773_001));
        assert_eq!(r1080.codecs.as_deref(), Some("avc1 + mp4a"));
        assert_eq!(r1080.resolution.as_deref(), Some("1920x1080"));
        // 720p: VP9 beats AV1.
        assert!(i.video[2].format.starts_with("247+140/"));
        // 360p is a combined format: no separate audio.
        assert!(i.video[3].format.starts_with("18/"));
        assert_eq!(i.audio.len(), 2);
        assert_eq!(i.audio[0].ext, "M4A");
        assert_eq!(i.audio[0].size, Some(3_773_001));
        assert_eq!(i.audio[1].extract_audio.as_deref(), Some("mp3"));
    }

    #[test]
    fn quality_settings() {
        assert_eq!(quality_option("1080").format, height_selector(1080));
        assert_eq!(quality_option("720p").height, Some(720));
        assert_eq!(quality_option("best").format, "bv*+ba/b");
        assert_eq!(quality_option("mp3").extract_audio.as_deref(), Some("mp3"));
        assert_eq!(quality_option("audio").kind, "audio");
        assert_eq!(quality_option("weird").format, "bv*+ba/b");
    }

    #[test]
    fn drm_only_is_refused() {
        let v = json!({"id": "x", "title": "Film", "formats": [{"format_id": "1", "vcodec": "avc1", "acodec": "none", "height": 1080, "has_drm": true}]});
        assert_eq!(parse_info(&v, "https://x.test/f"), Err(DlError::Drm));
    }

    #[test]
    fn playlists_list_entries() {
        let v = json!({"_type": "playlist", "id": "PL1", "title": "Mix", "extractor_key": "YoutubeTab", "entries": [
            {"id": "a", "title": "One", "url": "https://www.youtube.com/watch?v=a", "duration": 60.0},
            {"id": "b", "title": "Two", "url": "https://www.youtube.com/watch?v=b"}
        ]});
        let i = parse_info(&v, "https://www.youtube.com/playlist?list=PL1").unwrap();
        assert!(i.is_playlist);
        assert_eq!(i.entries.len(), 2);
        assert_eq!(i.platform, "YouTube");
        assert!(i.video.iter().any(|o| o.format == height_selector(1080)));
    }

    #[test]
    fn errors_are_typed() {
        let e = last_error("WARNING: x\nERROR: [youtube] abc: Video unavailable. This video is private\n").unwrap();
        assert_eq!(e, "Video unavailable. This video is private");
        assert_eq!(map_error(&e, None), DlError::Removed);
        assert_eq!(map_error("This video is unavailable", None), DlError::Removed);
        assert_eq!(map_error("Sign in to confirm you're not a bot", None), DlError::LoginRequired);
        assert_eq!(map_error("This video is DRM protected", None), DlError::Drm);
        assert_eq!(map_error("The uploader has not made this video available in your country", None), DlError::GeoBlocked);
        assert_eq!(map_error("HTTP Error 429: Too Many Requests", None), DlError::RateLimited);
        assert_eq!(map_error("Unable to extract uploader id", Some(120)), DlError::ToolOutdated("yt-dlp".into()));
        assert!(matches!(map_error("Unsupported URL: https://x", Some(3)), DlError::Other(_)));
        // An unsupported page is not fixed by updating, however old yt-dlp is.
        assert!(matches!(map_error("ERROR: Unsupported URL: https://myapi.agego.com/noJS", Some(90)), DlError::Other(_)));
        assert!(matches!(map_error("ERROR: Unable to extract nsig function", Some(90)), DlError::ToolOutdated(_)));
        assert!(matches!(map_error("Unable to download webpage: timed out", None), DlError::Network(_)));
    }

    #[test]
    fn progress_lines() {
        let p = parse_progress_line("ENDL downloading 1048576 66072637 NA 2500000.5 25 137").unwrap();
        assert_eq!(p, ProgressLine { format_id: "137".into(), done: 1_048_576, total: Some(66_072_637), speed: 2_500_000, eta: Some(25) });
        let p = parse_progress_line("ENDL downloading 500 NA 9000.0 NA NA 251").unwrap();
        assert_eq!(p, ProgressLine { format_id: "251".into(), done: 500, total: Some(9000), speed: 0, eta: None });
        let p = parse_progress_line("ENDL finished NA 11000000 NA NA NA 135").unwrap();
        assert_eq!((p.done, p.total), (11_000_000, Some(11_000_000)), "already-downloaded stream counts as done");
        assert!(parse_progress_line("[download] 10%").is_none());
    }

    #[test]
    fn stream_progress_sums_video_and_audio() {
        let mut a = StreamProgress::new(2, Some(70_000));
        a.update("137", 30_000, Some(66_000));
        assert_eq!((a.done(), a.total()), (30_000, Some(70_000)));
        a.update("137", 66_000, Some(66_000));
        a.update("140", 1_000, Some(4_000));
        assert_eq!((a.done(), a.total()), (67_000, Some(70_000)));
        let mut b = StreamProgress::new(1, None);
        b.update("18", 10, None);
        assert_eq!(b.total(), None, "unknown size stays unknown (indeterminate bar)");
    }

    #[test]
    fn finds_partial_files() {
        let d = tempfile::tempdir().unwrap();
        for n in ["Song [abc].f137.mp4.part", "Song [abc].f140.m4a", "Song [abc].mp4.ytdl", "Song [abc].mp4", "Other [xyz].mp4.part", "Song [abc].mp4.part-Frag3"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        let mut got: Vec<String> = partial_files(d.path(), "abc").iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        got.sort();
        assert_eq!(got, vec!["Song [abc].f137.mp4.part", "Song [abc].f140.m4a", "Song [abc].mp4.part-Frag3", "Song [abc].mp4.ytdl"]);
    }

    #[test]
    fn pages_versus_files() {
        for page in [
            "https://xhamster.com/videos/some-title-xh9AUAz",
            "https://example.com/watch?v=1",
            "https://example.com/",
            "https://news.example.com/2026/story.html",
            "https://example.com/view.php?id=7",
            "https://example.com/releases/v1.2-beta",
        ] {
            assert!(looks_like_page(page), "{page}");
        }
        for file in [
            "https://example.com/file.zip",
            "https://cdn.example.com/a/b/movie.MP4?token=x",
            "https://github.com/o/r/releases/download/v1/app_1.0_amd64.deb",
            "https://example.com/ubuntu-24.04.1-desktop-amd64.iso",
            "ftp://example.com/x",
        ] {
            assert!(!looks_like_page(file), "{file}");
        }
    }
}
