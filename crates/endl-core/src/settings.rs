//! Typed settings. Stored in SQLite as one JSON value per key (like ENViewer), so adding a
//! setting never needs a migration and unknown/invalid stored values fall back to defaults.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    /// Lowercase extensions without dots.
    pub extensions: Vec<String>,
    /// Absolute folder; None = a sub-folder named after the category in the download folder.
    pub folder: Option<String>,
}

fn cat(id: &str, name: &str, exts: &str) -> Category {
    Category { id: id.into(), name: name.into(), extensions: exts.split_whitespace().map(String::from).collect(), folder: None }
}

pub fn default_categories() -> Vec<Category> {
    vec![
        cat("video", "Video", "mp4 mkv webm mov avi wmv flv m4v mpg mpeg 3gp ts m2ts ogv"),
        cat("audio", "Audio", "mp3 m4a aac flac wav ogg opus wma aiff alac ape"),
        cat("images", "Images", "jpg jpeg png gif webp avif heic heif bmp tif tiff svg ico jxl raw dng"),
        cat("archives", "Archives", "zip rar 7z tar gz tgz bz2 xz zst lz cab"),
        cat("programs", "Programs", "exe msi deb rpm appimage apk dmg pkg msix snap flatpakref run sh bat"),
        cat("iso", "ISO", "iso img vhd vhdx vmdk qcow2"),
        cat("documents", "Documents", "pdf doc docx xls xlsx ppt pptx odt ods odp txt rtf csv epub md"),
        cat("other", "Other", ""),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // General
    pub start_with_os: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub confirm_remove: bool,
    pub notify_on_finish: bool,
    // Downloads
    /// Empty = the system Downloads folder.
    pub download_dir: String,
    pub max_parallel: u32,
    pub connections: u32,
    /// "rename" | "overwrite" | "skip" | "ask"
    pub conflict: String,
    pub detect_duplicates: bool,
    pub use_categories: bool,
    pub keep_partial: bool,
    /// Automatic retries for network errors (with growing waits).
    pub retries: u32,
    pub categories: Vec<Category>,
    // Appearance
    /// "dark" | "light" | "glass" | "system"
    pub theme: String,
    pub accent: String,
    pub animations: bool,
    pub sidebar_collapsed: bool,
    pub language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_os: false,
            start_minimized: false,
            close_to_tray: true,
            confirm_remove: true,
            notify_on_finish: true,
            download_dir: String::new(),
            max_parallel: 3,
            connections: 8,
            conflict: "rename".into(),
            detect_duplicates: true,
            use_categories: true,
            keep_partial: false,
            retries: 3,
            categories: default_categories(),
            theme: "dark".into(),
            accent: "#3b82f6".into(),
            animations: true,
            sidebar_collapsed: false,
            language: "en".into(),
        }
    }
}

pub const MAX_PARALLEL: u32 = 10;
pub const MAX_CONNECTIONS: u32 = 16;

impl Settings {
    /// Build from stored key → JSON pairs. Each key is applied on its own, so one bad
    /// value only resets that key.
    pub fn from_map(stored: &HashMap<String, Value>) -> Self {
        let defaults = Settings::default();
        let Ok(Value::Object(mut base)) = serde_json::to_value(&defaults) else { return defaults };
        for (k, v) in stored {
            if !base.contains_key(k) {
                continue;
            }
            let mut trial: Map<String, Value> = base.clone();
            trial.insert(k.clone(), v.clone());
            if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
                base = trial;
            } else {
                log::warn!("ignoring invalid stored setting {k}");
            }
        }
        serde_json::from_value::<Settings>(Value::Object(base)).unwrap_or(defaults).validated()
    }

    /// Clamp numbers and fix enums so the rest of the app can trust the values.
    pub fn validated(mut self) -> Self {
        self.max_parallel = self.max_parallel.clamp(1, MAX_PARALLEL);
        self.connections = self.connections.clamp(1, MAX_CONNECTIONS);
        self.retries = self.retries.min(10);
        if !matches!(self.conflict.as_str(), "rename" | "overwrite" | "skip" | "ask") {
            self.conflict = "rename".into();
        }
        if !matches!(self.theme.as_str(), "dark" | "light" | "glass" | "system") {
            self.theme = "dark".into();
        }
        if !matches!(self.language.as_str(), "en" | "bn") {
            self.language = "en".into();
        }
        for c in &mut self.categories {
            c.extensions = c.extensions.iter().map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
        }
        if !self.categories.iter().any(|c| c.id == "other") {
            self.categories.push(cat("other", "Other", ""));
        }
        self
    }

    /// Whether `key` names a known setting.
    pub fn is_known_key(key: &str) -> bool {
        matches!(serde_json::to_value(Settings::default()), Ok(Value::Object(m)) if m.contains_key(key))
    }

    /// Category id for a file name (by extension), "other" when nothing matches.
    pub fn category_for(&self, filename: &str) -> String {
        let ext = crate::filename::extension_lower(filename);
        if ext.is_empty() {
            return "other".into();
        }
        self.categories.iter().find(|c| c.extensions.contains(&ext)).map_or_else(|| "other".into(), |c| c.id.clone())
    }

    /// Where a file of `category` goes, given the resolved base download folder.
    pub fn folder_for(&self, base: &Path, category: &str) -> PathBuf {
        if !self.use_categories || category == "other" {
            return base.to_path_buf();
        }
        match self.categories.iter().find(|c| c.id == category) {
            Some(Category { folder: Some(f), .. }) if !f.trim().is_empty() => PathBuf::from(f),
            Some(c) => base.join(crate::filename::clean_filename(&c.name)),
            None => base.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_roundtrip() {
        let s = Settings::from_map(&HashMap::new());
        assert_eq!(s, Settings::default());
        assert_eq!(s.max_parallel, 3);
        assert_eq!(s.connections, 8);
    }

    #[test]
    fn bad_values_fall_back_per_key() {
        let mut m = HashMap::new();
        m.insert("maxParallel".to_string(), json!("lots"));
        m.insert("connections".to_string(), json!(64));
        m.insert("theme".to_string(), json!("light"));
        m.insert("unknownKey".to_string(), json!(1));
        let s = Settings::from_map(&m);
        assert_eq!(s.max_parallel, 3);
        assert_eq!(s.connections, 16);
        assert_eq!(s.theme, "light");
    }

    #[test]
    fn categories_by_extension() {
        let s = Settings::default();
        assert_eq!(s.category_for("ubuntu-24.04.iso"), "iso");
        assert_eq!(s.category_for("Movie.MKV"), "video");
        assert_eq!(s.category_for("setup.exe"), "programs");
        assert_eq!(s.category_for("archive.tar.gz"), "archives");
        assert_eq!(s.category_for("README"), "other");
        assert_eq!(s.category_for("thing.xyz"), "other");
    }

    #[test]
    fn folders() {
        let mut s = Settings::default();
        let base = Path::new("/home/u/Downloads");
        assert_eq!(s.folder_for(base, "video"), base.join("Video"));
        assert_eq!(s.folder_for(base, "other"), base);
        s.categories[0].folder = Some("/media/films".into());
        assert_eq!(s.folder_for(base, "video"), PathBuf::from("/media/films"));
        s.use_categories = false;
        assert_eq!(s.folder_for(base, "audio"), base);
    }

    #[test]
    fn extensions_normalized() {
        let mut s = Settings::default();
        s.categories[0].extensions = vec![".MP4".into(), " ".into()];
        let s = s.validated();
        assert_eq!(s.categories[0].extensions, vec!["mp4"]);
    }
}
