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

/// All links in a block of text (one per whitespace-separated token), in order, without
/// duplicates.
pub fn extract_links(text: &str) -> ExtractedLinks {
    let mut out = ExtractedLinks::default();
    for token in text.split_whitespace() {
        match normalize(token) {
            Some(Ok(l)) => {
                if !out.links.contains(&l) {
                    out.links.push(l);
                }
            }
            Some(Err(bad)) => out.invalid.push(bad),
            None => {}
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
    fn dedupe_key_normalizes() {
        assert_eq!(dedupe_key("https://www.Example.com/a/#top"), dedupe_key("https://example.com/a"));
        assert_ne!(dedupe_key("https://example.com/a?x=1"), dedupe_key("https://example.com/a?x=2"));
    }
}
