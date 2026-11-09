//! Live tests against real sites (network + yt-dlp + FFmpeg needed), so they are ignored by
//! default: `cargo test -p endl-core --test media_live -- --ignored --nocapture --test-threads=1`

use endl_core::db::{Db, NewTask};
use endl_core::media::{analyze, MediaEngine, MediaPrefs};
use endl_core::scheduler::{Engines, Notifier, ProgressItem, RunConfig, Scheduler};
use endl_core::task::{Action, EngineKind, Task, TaskStatus};
use endl_core::tools::{ToolKind, Tools};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LINK: &str = "https://www.youtube.com/watch?v=mk48xRzuNvA&list=RDmk48xRzuNvA&start_radio=1";

fn tools() -> Tools {
    Tools::new(std::env::temp_dir().join("endl-no-managed-tools"))
}

#[derive(Default)]
struct Rec(Mutex<Vec<ProgressItem>>);
impl Notifier for Rec {
    fn upsert(&self, _: &[Task]) {}
    fn removed(&self, _: &[i64]) {}
    fn progress(&self, items: &[ProgressItem]) {
        self.0.lock().extend_from_slice(items);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn youtube_analyze_and_download_with_pause() {
    let t = tools();
    assert!(t.path(ToolKind::YtDlp).is_some(), "yt-dlp not found");
    eprintln!("yt-dlp {:?}, js: {:?}", t.version(ToolKind::YtDlp).await, t.js_runtime_args());

    let started = Instant::now();
    let info = analyze(&t, &MediaPrefs::default(), LINK, false).await.expect("analyze");
    eprintln!("analyzed in {:.1}s: {} — {:?}", started.elapsed().as_secs_f64(), info.title, info.thumbnail);
    for o in info.video.iter().chain(&info.audio) {
        eprintln!("  {:<14} {:<4} {:<10} {:<14} {:>12?} {}", o.label, o.ext, o.resolution.clone().unwrap_or_default(), o.codecs.clone().unwrap_or_default(), o.size, o.format);
    }
    assert!(info.title.contains("Hall of Fame"));
    assert!(info.thumbnail.is_some());
    assert!(info.video.iter().any(|o| o.height == Some(1080)));
    assert_eq!(info.playlist_id.as_deref(), Some("RDmk48xRzuNvA"));

    // Download 480p (video + audio merged) through the real scheduler, pausing midway.
    let opt = info.video.iter().find(|o| o.height == Some(480)).expect("480p").clone();
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Db::open_in_memory().unwrap());
    let engine = MediaEngine::new(t.clone(), Arc::new(MediaPrefs::default));
    let rec = Arc::new(Rec::default());
    let (s, h) = Scheduler::new(db.clone(), Engines::new(vec![engine]), rec.clone(), Arc::new(RunConfig::default));
    tokio::spawn(s.run());
    let ids: Vec<i64> = db
        .insert_tasks(&[NewTask {
            url: LINK.into(),
            engine: EngineKind::Media,
            title: Some(info.title.clone()),
            folder: dir.path().to_string_lossy().into_owned(),
            category: "video".into(),
            connections: 1,
            options: serde_json::json!({ "format": opt.format, "merge": opt.merge, "size": opt.size, "mediaId": info.id }),
            ..Default::default()
        }])
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    h.kick();
    let status = |db: &Db| db.get_task(ids[0]).unwrap().unwrap();
    let t0 = Instant::now();
    while rec.0.lock().last().map_or(0, |p| p.done) < 2_000_000 {
        assert!(t0.elapsed() < Duration::from_secs(60), "no progress: {:?}", status(&db));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let before = rec.0.lock().last().cloned().unwrap();
    eprintln!("progress before pause: {before:?}");
    h.act(ids.clone(), Action::Pause).await.unwrap();
    assert_eq!(status(&db).status, TaskStatus::Paused);
    let parts: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).collect();
    eprintln!("files while paused: {parts:?}");
    let finished_before: u64 = parts
        .iter()
        .filter(|n| !n.to_string_lossy().ends_with(".part"))
        .filter_map(|n| std::fs::metadata(dir.path().join(n)).ok())
        .map(|m| m.len())
        .sum();
    // On a fast link the video stream may already be finished (`name.f135.mp4`) with the audio not
    // started yet, so there is no `.part` file; a finished stream is kept progress too.
    let is_partial = |n: &std::ffi::OsString| {
        let n = n.to_string_lossy();
        n.ends_with(".part") || n.contains(&format!(".f{}", opt.format.split('+').next().unwrap_or_default()))
    };
    assert!(parts.iter().any(is_partial), "partial data kept: {parts:?}");
    tokio::time::sleep(Duration::from_secs(1)).await;
    h.act(ids.clone(), Action::Resume).await.unwrap();
    while status(&db).status != TaskStatus::Completed {
        let t = status(&db);
        assert!(t.status != TaskStatus::Failed, "failed: {:?}", t.error);
        assert!(t0.elapsed() < Duration::from_secs(240), "timeout: {t:?}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let t = status(&db);
    let name = t.filename.clone().unwrap();
    let path = dir.path().join(&name);
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!("done in {:.1}s: {name} ({size} bytes, estimate {:?})", t0.elapsed().as_secs_f64(), opt.size);
    assert!(name.ends_with(".mp4"), "{name}");
    assert!(size > 5_000_000);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name()).filter(|n| n.to_string_lossy() != name).collect();
    assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
    let first_after = rec.0.lock().iter().skip_while(|p| p.done != before.done).nth(2).cloned();
    eprintln!("first progress after resume: {first_after:?}");
    // Resuming must not send the bar back: streams finished before the pause still count.
    let low = rec.0.lock().iter().skip_while(|p| p.done != before.done).skip(1).map(|p| p.done).min();
    assert!(low.unwrap_or(u64::MAX) >= finished_before, "progress dropped after resume: {low:?} < {finished_before}");
    h.shutdown().await;
}

/// Runs one media task through the real scheduler to the end; returns the finished task.
async fn download(opt: &endl_core::media::MediaOption, url: &str, media_id: &str, dir: &std::path::Path) -> Task {
    let db = Arc::new(Db::open_in_memory().unwrap());
    let engine = MediaEngine::new(tools(), Arc::new(MediaPrefs::default));
    let (s, h) = Scheduler::new(db.clone(), Engines::new(vec![engine]), Arc::new(Rec::default()), Arc::new(RunConfig::default));
    tokio::spawn(s.run());
    let id = db
        .insert_tasks(&[NewTask {
            url: url.into(),
            engine: EngineKind::Media,
            folder: dir.to_string_lossy().into_owned(),
            category: opt.kind.clone(),
            connections: 1,
            options: serde_json::json!({ "format": opt.format, "merge": opt.merge, "extractAudio": opt.extract_audio, "size": opt.size, "mediaId": media_id }),
            ..Default::default()
        }])
        .unwrap()[0]
        .id;
    h.kick();
    let t0 = Instant::now();
    loop {
        let t = db.get_task(id).unwrap().unwrap();
        if matches!(t.status, TaskStatus::Completed | TaskStatus::Failed) {
            h.shutdown().await;
            return t;
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "timeout: {t:?}");
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn youtube_mp3_extract() {
    let opt = endl_core::media::quality_option("mp3");
    let dir = tempfile::tempdir().unwrap();
    let t = download(&opt, "https://www.youtube.com/watch?v=mk48xRzuNvA", "mk48xRzuNvA", dir.path()).await;
    assert_eq!(t.status, TaskStatus::Completed, "failed: {:?}", t.error);
    let name = t.filename.clone().unwrap();
    let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    eprintln!("mp3: {name}, folder: {files:?}");
    assert!(name.ends_with(".mp3"), "{name}");
    assert_eq!(files, vec![name.clone()], "only the mp3 is left");
    let bytes = std::fs::read(dir.path().join(&name)).unwrap();
    assert!(bytes.len() > 1_000_000);
    assert!(bytes.starts_with(b"ID3") || (bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0), "not an MP3 file");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn youtube_playlist_entries() {
    let info = analyze(&tools(), &MediaPrefs::default(), LINK, true).await.expect("analyze playlist");
    eprintln!("playlist {:?}: {} entries; first: {:?}", info.playlist_id, info.entries.len(), info.entries.first().map(|e| &e.title));
    assert!(info.is_playlist);
    assert!(info.entries.len() > 5, "{} entries", info.entries.len());
    for e in &info.entries {
        assert!(!e.id.is_empty() && !e.title.is_empty(), "{e:?}");
        assert!(e.url.starts_with("https://"), "{e:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn youtube_unavailable_video_is_typed_error() {
    let err = analyze(&tools(), &MediaPrefs::default(), "https://www.youtube.com/watch?v=aaaaaaaaaaa", false).await.unwrap_err();
    eprintln!("unavailable video: {err:?} / {err}");
    assert_eq!(err, endl_core::error::DlError::Removed);
}
