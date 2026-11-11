//! "Save as one ZIP" for bulk downloads (a profile, channel or playlist).
//!
//! ```text
//!  add_media(zip = "farihamadha") ──▶ every task downloads into  <folder>/farihamadha/
//!                                      options.zip = { batch, name, dir }
//!  a task of the batch ends ──▶ all ended? ──▶ zip_folder(dir → <folder>/farihamadha.zip)
//!                                              packed files and empty folders are removed
//! ```
//! Retrying a failed item later downloads into the same folder again; when the batch ends
//! once more, the new files are added to the existing ZIP.

use crate::error::DlError;
use crate::task::{Task, TaskStatus};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// The ZIP batch a task belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub id: String,
    /// ZIP file name without ".zip".
    pub name: String,
    /// The folder the batch downloads into.
    pub dir: PathBuf,
}

impl Batch {
    pub fn of(t: &Task) -> Option<Batch> {
        let z = t.options.get("zip")?;
        let s = |k: &str| z.get(k).and_then(|v| v.as_str()).map(str::to_string).filter(|v| !v.is_empty());
        Some(Batch { id: s("batch")?, name: s("name")?, dir: PathBuf::from(s("dir")?) })
    }

    /// Where the ZIP goes: next to the batch folder.
    pub fn zip_path(&self) -> PathBuf {
        let parent = self.dir.parent().unwrap_or(&self.dir);
        parent.join(format!("{}.zip", self.name))
    }
}

/// Every task of the batch has ended (completed, failed or cancelled), and at least one
/// completed. A paused task keeps the batch open.
pub fn batch_finished(tasks: &[Task], batch_id: &str) -> bool {
    let mine: Vec<&Task> = tasks.iter().filter(|t| Batch::of(t).is_some_and(|b| b.id == batch_id)).collect();
    !mine.is_empty()
        && mine.iter().all(|t| matches!(t.status, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled))
        && mine.iter().any(|t| t.status == TaskStatus::Completed)
}

fn is_partial(name: &str) -> bool {
    name.ends_with(".part") || name.ends_with(".ytdl") || name.contains(".part-Frag") || name.ends_with(".temp") || name.ends_with(".aria2")
}

/// Finished files under `dir`, as (path, name inside the ZIP with '/' separators).
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type()?;
        if ft.is_dir() {
            collect(&e.path(), &format!("{prefix}{name}/"), out)?;
        } else if ft.is_file() && !is_partial(&name) {
            out.push((e.path(), format!("{prefix}{name}")));
        }
    }
    Ok(())
}

/// "a.mp4" → "a (2).mp4" until the name is free.
fn free_name(name: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(name) {
        return name.to_string();
    }
    let (dir, file) = name.rsplit_once('/').map_or(("", name), |(d, f)| (d, f));
    let (stem, ext) = file.rsplit_once('.').map_or((file, ""), |(s, e)| (s, e));
    (2..)
        .map(|n| {
            let f = if ext.is_empty() { format!("{stem} ({n})") } else { format!("{stem} ({n}).{ext}") };
            if dir.is_empty() { f } else { format!("{dir}/{f}") }
        })
        .find(|c| !taken.contains(c))
        .unwrap_or_else(|| name.to_string())
}

/// Pack the finished files under `dir` into `zip` (stored, not compressed: videos and
/// pictures are compressed already), adding to it when it exists. The packed files and
/// the folders left empty are removed. Returns how many files were added.
pub fn zip_folder(dir: &Path, zip: &Path) -> Result<usize, DlError> {
    let mut files = Vec::new();
    collect(dir, "", &mut files)?;
    if files.is_empty() {
        return Ok(0);
    }
    let bad = |e: zip::result::ZipError| DlError::Other(format!("could not write {}: {e}", zip.display()));
    let mut taken = HashSet::new();
    let mut writer = if zip.is_file() {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(zip)?;
        let w = zip::ZipWriter::new_append(f).map_err(bad)?;
        // Names already in the archive: new files get "(2)" instead of a duplicate entry.
        let mut a = zip::ZipArchive::new(BufReader::new(File::open(zip)?)).map_err(bad)?;
        for i in 0..a.len() {
            if let Ok(e) = a.by_index_raw(i) {
                taken.insert(e.name().to_string());
            }
        }
        w
    } else {
        let tmp_parent = zip.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(tmp_parent)?;
        zip::ZipWriter::new(File::create(zip)?)
    };
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
    for (path, name) in &files {
        let entry = free_name(name, &taken);
        taken.insert(entry.clone());
        writer.start_file(entry, opts).map_err(bad)?;
        let mut src = BufReader::new(File::open(path)?);
        std::io::copy(&mut src, &mut writer)?;
    }
    let inner = writer.finish().map_err(bad)?;
    let mut out = BufWriter::new(inner);
    std::io::Write::flush(&mut out)?;
    drop(out);
    for (path, _) in &files {
        let _ = std::fs::remove_file(path);
    }
    remove_empty_dirs(dir);
    Ok(files.len())
}

/// Remove `dir` and the folders inside it that are empty (bottom-up).
fn remove_empty_dirs(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                remove_empty_dirs(&e.path());
            }
        }
    }
    let _ = std::fs::remove_dir(dir); // fails (and is kept) when not empty
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn names(zip: &Path) -> Vec<String> {
        let mut a = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
        let mut v: Vec<String> = (0..a.len()).map(|i| a.by_index(i).unwrap().name().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn packs_finished_files_and_appends_later() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("farihamadha");
        std::fs::create_dir_all(dir.join("বৈশাখ [1]")).unwrap();
        std::fs::write(dir.join("clip [2].mp4"), b"video").unwrap();
        std::fs::write(dir.join("বৈশাখ [1]/01.jpg"), b"pic1").unwrap();
        std::fs::write(dir.join("বৈশাখ [1]/02.jpg"), b"pic2").unwrap();
        std::fs::write(dir.join("broken [3].mp4.part"), b"half").unwrap();
        let zip = base.path().join("farihamadha.zip");

        assert_eq!(zip_folder(&dir, &zip).unwrap(), 3);
        assert_eq!(names(&zip), vec!["clip [2].mp4", "বৈশাখ [1]/01.jpg", "বৈশাখ [1]/02.jpg"]);
        let mut a = zip::ZipArchive::new(File::open(&zip).unwrap()).unwrap();
        let mut s = String::new();
        a.by_name("clip [2].mp4").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "video");
        // Packed files are gone; the partial file (a failed item) and its folder stay.
        assert!(!dir.join("clip [2].mp4").exists() && !dir.join("বৈশাখ [1]").exists());
        assert!(dir.join("broken [3].mp4.part").exists());

        // A retried item finishes later: it is added, a clashing name gets "(2)".
        std::fs::remove_file(dir.join("broken [3].mp4.part")).unwrap();
        std::fs::write(dir.join("broken [3].mp4"), b"whole").unwrap();
        std::fs::write(dir.join("clip [2].mp4"), b"again").unwrap();
        assert_eq!(zip_folder(&dir, &zip).unwrap(), 2);
        assert_eq!(names(&zip), vec!["broken [3].mp4", "clip [2] (2).mp4", "clip [2].mp4", "বৈশাখ [1]/01.jpg", "বৈশাখ [1]/02.jpg"]);
        assert!(!dir.exists(), "the empty batch folder is removed");
        assert_eq!(zip_folder(&dir, &zip).unwrap_or(0), 0);
    }

    #[test]
    fn batch_ends_when_every_item_has_ended() {
        use crate::db::{Db, NewTask};
        use crate::task::{Action, EngineKind};
        let db = Db::open_in_memory().unwrap();
        let zip = serde_json::json!({ "zip": { "batch": "b1", "name": "farihamadha", "dir": "/d/farihamadha" } });
        let new: Vec<NewTask> = (0..3)
            .map(|i| NewTask { url: format!("https://www.tiktok.com/@u/video/{i}"), engine: EngineKind::Media, folder: "/d/farihamadha".into(), category: "video".into(), connections: 1, options: zip.clone(), ..Default::default() })
            .chain(std::iter::once(NewTask { url: "https://example.com/other.zip".into(), folder: "/d".into(), category: "archives".into(), connections: 1, ..Default::default() }))
            .collect();
        let ids: Vec<i64> = db.insert_tasks(&new).unwrap().into_iter().map(|t| t.id).collect();
        let b = Batch::of(&db.get_task(ids[0]).unwrap().unwrap()).unwrap();
        assert_eq!(b.zip_path(), PathBuf::from("/d/farihamadha.zip"));
        assert!(Batch::of(&db.get_task(ids[3]).unwrap().unwrap()).is_none());
        let finish = |id: i64| {
            db.apply_action(&[id], Action::Start).unwrap();
            db.apply_action(&[id], Action::Run).unwrap();
            db.apply_action(&[id], Action::Complete).unwrap();
        };
        finish(ids[0]);
        assert!(!batch_finished(&db.list_tasks().unwrap(), "b1"), "two still queued");
        db.apply_action(&[ids[1]], Action::Start).unwrap();
        assert!(db.set_failed(ids[1], &DlError::Forbidden).unwrap().is_some());
        finish(ids[2]);
        assert!(batch_finished(&db.list_tasks().unwrap(), "b1"), "completed + failed = ended");
        assert!(!batch_finished(&db.list_tasks().unwrap(), "other"));
    }

    #[test]
    fn free_names() {
        let taken: HashSet<String> = ["a.mp4", "a (2).mp4", "d/b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(free_name("a.mp4", &taken), "a (3).mp4");
        assert_eq!(free_name("d/b", &taken), "d/b (2)");
        assert_eq!(free_name("new.jpg", &taken), "new.jpg");
    }
}
