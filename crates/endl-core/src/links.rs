//! Turning pasted text into links: split on any whitespace, strip surrounding punctuation,
//! add a missing `https://`, validate, and drop duplicates.

use url::Url;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExtractedLinks {
    pub links: Vec<String>,
    /// Tokens that looked like links but could not be parsed.
    pub invalid: Vec<String>,
}

const SCHEMES: &[&str] = &["http", "https", "ftp", "ftps", "sftp"];

/// Normalize one token into a link, or None if it is clearly not one.
/// Err(token) when it looks like a link but is malformed.
pub fn normalize(token: &str) -> Option<Result<String, String>> {
    let t = token
        .trim()
        .trim_start_matches(['<', '(', '[', '"', '\'', '`'])
        .trim_end_matches(['>', ')', ']', '"', '\'', '`', ',', ';', '.', '!', '?']);
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("magnet:?") {
        return Some(Ok(t.to_string()));
    }
    let candidate = if let Some(idx) = lower.find("://") {
        let scheme = &lower[..idx];
        if !SCHEMES.contains(&scheme) {
            return None;
        }
        t.to_string()
    } else if lower.starts_with("//") {
        format!("https:{t}")
    } else if looks_like_domain(t) {
        format!("https://{t}")
    } else {
        return None;
    };
    match Url::parse(&candidate) {
        Ok(u) if u.host_str().is_some_and(|h| !h.is_empty()) => Some(Ok(u.to_string())),
        _ => Some(Err(t.to_string())),
    }
}

/// `example.com/x`, `www.site.org`, `192.168.1.5:8080/file` — a host with a dot and a
/// plausible top-level part, optionally followed by a port/path.
fn looks_like_domain(t: &str) -> bool {
    let host_end = t.find(['/', '?', '#']).unwrap_or(t.len());
    let host_port = &t[..host_end];
    let host = host_port.rsplit_once(':').map_or(host_port, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host_port });
    if host.contains('@') || !host.contains('.') || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    if !host.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-') {
        return false;
    }
    let tld = host.rsplit('.').next().unwrap_or("");
    let is_ip = host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok());
    // A file name like "notes.txt" alone is not a link; require a known-looking TLD length
    // and either a path, a www. prefix or at least two labels before an alphabetic TLD.
    is_ip || (tld.len() >= 2 && tld.chars().all(|c| c.is_alphabetic()) && (host_end < t.len() || host.starts_with("www.") || host.split('.').count() >= 2) && !is_probably_filename(host))
}

fn is_probably_filename(host: &str) -> bool {
    const FILE_EXTS: &[&str] = &["txt", "zip", "exe", "pdf", "mp4", "mp3", "jpg", "jpeg", "png", "gif", "iso", "rar", "7z", "doc", "docx", "json", "md", "rs", "ts", "js"];
    let ext = host.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    host.split('.').count() == 2 && FILE_EXTS.contains(&ext.as_str())
}

/// Most links one range pattern may expand to.
pub const MAX_EXPANSION: usize = 10_000;

/// One `[a-b]` range found in a link.
struct Range {
    start: usize,
    end: usize,
    values: Vec<String>,
}

/// Parse `[001-120]` (numbers, zero padding kept, may count down) or `[a-f]` (single
/// letters) at `open`.
fn parse_range(s: &str, open: usize) -> Option<Range> {
    let close = open + s[open..].find(']')?;
    let (a, b) = s[open + 1..close].split_once('-')?;
    let values: Vec<String> = if !a.is_empty() && !b.is_empty() && a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) {
        let (x, y): (u64, u64) = (a.parse().ok()?, b.parse().ok()?);
        let width = if a.len() > 1 && a.starts_with('0') || b.len() > 1 && b.starts_with('0') { a.len().max(b.len()) } else { 0 };
        let n = x.abs_diff(y) as usize + 1;
        if n > MAX_EXPANSION {
            return Some(Range { start: open, end: close + 1, values: vec![String::new(); MAX_EXPANSION + 1] });
        }
        let nums: Vec<u64> = if x <= y { (x..=y).collect() } else { (y..=x).rev().collect() };
        nums.into_iter().map(|v| format!("{v:0width$}")).collect()
    } else {
        let (ca, cb) = (single_letter(a)?, single_letter(b)?);
        if ca.is_ascii_lowercase() != cb.is_ascii_lowercase() {
            return None;
        }
        let (lo, hi) = (ca.min(cb) as u8, ca.max(cb) as u8);
        let mut v: Vec<String> = (lo..=hi).map(|c| (c as char).to_string()).collect();
        if ca > cb {
            v.reverse();
        }
        v
    };
    Some(Range { start: open, end: close + 1, values })
}

fn single_letter(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    (it.next().is_none() && c.is_ascii_alphabetic()).then_some(c)
}

/// Expand range patterns in the path/query of a link: `https://x.com/img[001-120].jpg`
/// becomes img001.jpg … img120.jpg; several ranges multiply. Brackets in the host part
/// (IPv6 addresses) are left alone. Err when the result would exceed [`MAX_EXPANSION`].
pub fn expand_ranges(link: &str) -> Result<Vec<String>, String> {
    let path_start = link.find("://").map_or(0, |i| link[i + 3..].find('/').map_or(link.len(), |p| i + 3 + p));
    let mut ranges = Vec::new();
    let mut i = path_start;
    while let Some(off) = link[i..].find('[') {
        let open = i + off;
        match parse_range(link, open) {
            Some(r) => {
                i = r.end;
                ranges.push(r);
            }
            None => i = open + 1,
        }
    }
    if ranges.is_empty() {
        return Ok(vec![link.to_string()]);
    }
    let total = ranges.iter().try_fold(1usize, |acc, r| acc.checked_mul(r.values.len()).filter(|n| *n <= MAX_EXPANSION));
    if total.is_none() {
        return Err(format!("{link} (more than {MAX_EXPANSION} links)"));
    }
    let mut out = vec![String::new()];
    let mut last = 0;
    for r in &ranges {
        let lit = &link[last..r.start];
        out = out.iter().flat_map(|prefix| r.values.iter().map(move |v| format!("{prefix}{lit}{v}"))).collect();
        last = r.end;
    }
    let tail = &link[last..];
    Ok(out.into_iter().map(|p| p + tail).collect())
}

/// All links in a block of text (one per whitespace-separated token), in order, without
/// duplicates. Range patterns (`file[01-20].jpg`) are expanded.
pub fn extract_links(text: &str) -> ExtractedLinks {
    let mut out = ExtractedLinks::default();
    let mut seen = std::collections::HashSet::new();
    for token in text.split_whitespace() {
        let expanded = if token.contains('[') {
            match expand_ranges(token) {
                Ok(v) => v,
                Err(bad) => {
                    out.invalid.push(bad);
                    continue;
                }
            }
        } else {
            vec![token.to_string()]
        };
        for t in expanded {
            match normalize(&t) {
                Some(Ok(l)) => {
                    if seen.insert(l.clone()) {
                        out.links.push(l);
                    }
                }
                Some(Err(bad)) => out.invalid.push(bad),
                None => {}
            }
        }
    }
    out
}

/// Key for duplicate-link detection: lowercase host without `www.`, no fragment, no
/// trailing slash.
pub fn dedupe_key(link: &str) -> String {
    match Url::parse(link) {
        Ok(mut u) => {
            u.set_fragment(None);
            if let Some(h) = u.host_str().map(|h| h.trim_start_matches("www.").to_ascii_lowercase()) {
                let _ = u.set_host(Some(&h));
            }
            u.to_string().trim_end_matches('/').to_string()
        }
        Err(_) => link.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_links_any_whitespace() {
        let r = extract_links("https://a.com/x.zip\nhttps://b.org/y.iso\t  https://c.net/z\r\n");
        assert_eq!(r.links, vec!["https://a.com/x.zip", "https://b.org/y.iso", "https://c.net/z"]);
    }

    #[test]
    fn adds_missing_scheme() {
        assert_eq!(normalize("example.com/file.zip"), Some(Ok("https://example.com/file.zip".into())));
        assert_eq!(normalize("www.youtube.com/watch?v=abc"), Some(Ok("https://www.youtube.com/watch?v=abc".into())));
        assert_eq!(normalize("youtu.be/abc"), Some(Ok("https://youtu.be/abc".into())));
        assert_eq!(normalize("//cdn.site.io/a.png"), Some(Ok("https://cdn.site.io/a.png".into())));
        assert_eq!(normalize("192.168.1.5:8080/a.iso"), Some(Ok("https://192.168.1.5:8080/a.iso".into())));
    }

    #[test]
    fn strips_punctuation_and_ignores_words() {
        let r = extract_links("Get it here: <https://a.com/f.zip>, or (https://b.com/g).  Thanks!");
        assert_eq!(r.links, vec!["https://a.com/f.zip", "https://b.com/g"]);
        assert!(r.invalid.is_empty());
    }

    #[test]
    fn plain_file_names_are_not_links() {
        assert_eq!(normalize("notes.txt"), None);
        assert_eq!(normalize("hello"), None);
        assert_eq!(normalize("v1.2"), None);
    }

    #[test]
    fn magnet_and_other_schemes() {
        assert!(matches!(normalize("magnet:?xt=urn:btih:abc"), Some(Ok(_))));
        assert_eq!(normalize("ftp://host.com/a"), Some(Ok("ftp://host.com/a".into())));
        assert_eq!(normalize("javascript://alert(1)"), None);
        assert_eq!(normalize("file:///etc/passwd"), None);
    }

    #[test]
    fn removes_duplicates() {
        let r = extract_links("https://a.com/x https://a.com/x");
        assert_eq!(r.links.len(), 1);
    }

    #[test]
    fn range_patterns_expand() {
        let r = extract_links("https://x.com/img[001-003].jpg");
        assert_eq!(r.links, vec!["https://x.com/img001.jpg", "https://x.com/img002.jpg", "https://x.com/img003.jpg"]);
        let r = extract_links("https://x.com/p[8-10]/[a-b].png");
        assert_eq!(r.links.len(), 6);
        assert_eq!(r.links[0], "https://x.com/p8/a.png");
        assert_eq!(r.links[5], "https://x.com/p10/b.png");
        assert_eq!(expand_ranges("https://x.com/[3-1].z").unwrap(), vec!["https://x.com/3.z", "https://x.com/2.z", "https://x.com/1.z"]);
        // Not ranges: IPv6 host, text in brackets, mixed letter case.
        assert_eq!(expand_ranges("http://[::1]:8080/a").unwrap(), vec!["http://[::1]:8080/a"]);
        assert_eq!(expand_ranges("https://x.com/[draft]-v2.pdf").unwrap().len(), 1);
        assert_eq!(expand_ranges("https://x.com/[a-Z].png").unwrap().len(), 1);
    }

    #[test]
    fn huge_ranges_are_rejected() {
        let r = extract_links("https://x.com/[1-99999].jpg");
        assert!(r.links.is_empty());
        assert_eq!(r.invalid.len(), 1);
        assert!(expand_ranges("https://x.com/[1-200]/[1-200]").is_err());
    }

    #[test]
    fn dedupe_key_normalizes() {
        assert_eq!(dedupe_key("https://www.Example.com/a/#top"), dedupe_key("https://example.com/a"));
        assert_ne!(dedupe_key("https://example.com/a?x=1"), dedupe_key("https://example.com/a?x=2"));
    }
}
