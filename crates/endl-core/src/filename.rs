//! File names: cleaning names from servers and titles so they are valid on Windows and
//! Linux, and picking a free name when the target exists.

use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 200;
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
    "LPT7", "LPT8", "LPT9",
];

/// Make `name` safe as a single path component on every supported OS.
/// Illegal characters become `_`, whitespace runs collapse, trailing dots/spaces go,
/// reserved Windows names get a `_` prefix, and the result fits in 200 bytes with the
/// extension kept.
pub fn clean_filename(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    let mut last_space = false;
    for c in name.chars() {
        let c = match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => ' ',
            c => c,
        };
        if c.is_whitespace() {
            if !last_space {
                s.push(' ');
            }
            last_space = true;
        } else {
            s.push(c);
            last_space = false;
        }
    }
    let mut s = s.trim().trim_end_matches(['.', ' ']).trim_start_matches(' ').to_string();
    if s.is_empty() || s.chars().all(|c| c == '.' || c == '_') {
        s = "download".into();
    }
    let stem_upper = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        s.insert(0, '_');
    }
    truncate_keep_ext(&s, MAX_BYTES)
}

fn truncate_keep_ext(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let (stem, ext) = split_ext(s);
    let ext_part = if ext.is_empty() || ext.len() > 16 { String::new() } else { format!(".{ext}") };
    let budget = max.saturating_sub(ext_part.len());
    let mut cut = String::new();
    for c in stem.chars() {
        if cut.len() + c.len_utf8() > budget {
            break;
        }
        cut.push(c);
    }
    format!("{}{}", cut.trim_end_matches(['.', ' ']), ext_part)
}

/// ("archive.tar", "gz") style split on the last dot; dotfiles have no extension.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

pub fn extension_lower(name: &str) -> String {
    split_ext(name).1.to_ascii_lowercase()
}

/// File name from the last path segment of a URL (percent-decoded), if it has one.
pub fn name_from_url(link: &str) -> Option<String> {
    let u = url::Url::parse(link).ok()?;
    let seg = u.path_segments()?.rev().find(|s| !s.is_empty())?;
    let decoded = percent_decode(seg);
    let cleaned = clean_filename(&decoded);
    (cleaned != "download").then_some(cleaned)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// File name from a `Content-Disposition` header (`filename*=` preferred over `filename=`).
pub fn name_from_content_disposition(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("filename*=").map(|_| &part[10..]) {
            // RFC 5987: charset'lang'value
            let value = v.trim_matches('"');
            let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
            return Some(clean_filename(&percent_decode(encoded)));
        }
        if lower.starts_with("filename=") {
            plain = Some(clean_filename(part[9..].trim().trim_matches('"')));
        }
    }
    plain
}

/// `name.ext`, `name (1).ext`, `name (2).ext` … — the first that does not exist in `dir`.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = split_ext(name);
    for n in 1..10_000 {
        let candidate = if ext.is_empty() { format!("{stem} ({n})") } else { format!("{stem} ({n}).{ext}") };
        let p = dir.join(candidate);
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn illegal_characters() {
        assert_eq!(clean_filename("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(clean_filename("  My   Video\t\n Title  "), "My Video Title");
        assert_eq!(clean_filename("name. . ."), "name");
        assert_eq!(clean_filename(""), "download");
        assert_eq!(clean_filename("..."), "download");
    }

    #[test]
    fn reserved_windows_names() {
        assert_eq!(clean_filename("CON"), "_CON");
        assert_eq!(clean_filename("nul.txt"), "_nul.txt");
        assert_eq!(clean_filename("console.txt"), "console.txt");
    }

    #[test]
    fn long_names_keep_extension() {
        let long = format!("{}.mp4", "ব".repeat(150)); // 3 bytes each
        let c = clean_filename(&long);
        assert!(c.len() <= 200, "{}", c.len());
        assert!(c.ends_with(".mp4"));
    }

    #[test]
    fn unicode_kept() {
        assert_eq!(clean_filename("গান – 🎵 song.mp3"), "গান – 🎵 song.mp3");
    }

    #[test]
    fn from_url_and_disposition() {
        assert_eq!(name_from_url("https://x.com/files/ubuntu%2024.04.iso?dl=1").as_deref(), Some("ubuntu 24.04.iso"));
        assert_eq!(name_from_url("https://x.com/"), None);
        assert_eq!(name_from_content_disposition("attachment; filename=\"report.pdf\"").as_deref(), Some("report.pdf"));
        assert_eq!(name_from_content_disposition("attachment; filename=a.pdf; filename*=UTF-8''%E0%A6%97.pdf").as_deref(), Some("গ.pdf"));
        assert_eq!(name_from_content_disposition("inline").as_deref(), None);
    }

    #[test]
    fn unique_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(dir.path(), "a.zip"), dir.path().join("a.zip"));
        std::fs::write(dir.path().join("a.zip"), b"").unwrap();
        assert_eq!(unique_path(dir.path(), "a.zip"), dir.path().join("a (1).zip"));
        std::fs::write(dir.path().join("a (1).zip"), b"").unwrap();
        assert_eq!(unique_path(dir.path(), "a.zip"), dir.path().join("a (2).zip"));
    }
}
