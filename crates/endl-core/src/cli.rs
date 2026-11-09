//! Command line: `endownloader "<url>" [more urls] [--folder X] [--quality 1080] [--minimized]`.
//! A second launch forwards the same arguments to the running instance.

use serde::Serialize;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRequest {
    pub urls: Vec<String>,
    pub folder: Option<String>,
    /// Video height preset for media links (e.g. 1080), or "audio".
    pub quality: Option<String>,
    /// Start hidden in the tray (used by "start with OS").
    pub minimized: bool,
}

impl CliRequest {
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty() && self.folder.is_none() && self.quality.is_none()
    }
}

/// Parse arguments without the program name.
pub fn parse_args<I, S>(args: I) -> CliRequest
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut req = CliRequest::default();
    let mut it = args.into_iter().map(|s| s.as_ref().to_string()).peekable();
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = |inline: Option<String>| inline.or_else(|| it.next_if(|n| !n.starts_with("--")));
        match flag.as_str() {
            "--folder" | "-o" => req.folder = value(inline).filter(|v| !v.is_empty()),
            "--quality" | "-q" => req.quality = value(inline).map(|v| v.trim_end_matches(['p', 'P']).to_string()).filter(|v| !v.is_empty()),
            "--minimized" | "--hidden" => req.minimized = true,
            f if f.starts_with('-') => {} // unknown flags (and platform noise like -psn_…) are ignored
            _ => {
                for l in crate::links::extract_links(&a).links {
                    if !req.urls.contains(&l) {
                        req.urls.push(l);
                    }
                }
            }
        }
    }
    req
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_folder_quality() {
        let r = parse_args(["https://youtu.be/abc", "--folder", "/tmp/x", "--quality", "1080"]);
        assert_eq!(r.urls, vec!["https://youtu.be/abc"]);
        assert_eq!(r.folder.as_deref(), Some("/tmp/x"));
        assert_eq!(r.quality.as_deref(), Some("1080"));
    }

    #[test]
    fn inline_values_and_suffix() {
        let r = parse_args(["--quality=720p", "--folder=C:\\Users\\me\\Downloads", "example.com/a.zip"]);
        assert_eq!(r.quality.as_deref(), Some("720"));
        assert_eq!(r.folder.as_deref(), Some("C:\\Users\\me\\Downloads"));
        assert_eq!(r.urls, vec!["https://example.com/a.zip"]);
    }

    #[test]
    fn minimized_and_unknown_flags() {
        let r = parse_args(["--minimized", "--weird", "-psn_0_1234"]);
        assert!(r.minimized);
        assert!(r.urls.is_empty());
        assert!(r.is_empty());
    }

    #[test]
    fn missing_value_does_not_eat_next_flag() {
        let r = parse_args(["--folder", "--minimized"]);
        assert_eq!(r.folder, None);
        assert!(r.minimized);
    }

    #[test]
    fn non_links_ignored() {
        let r = parse_args(["hello", "https://a.com/x", "https://a.com/x"]);
        assert_eq!(r.urls, vec!["https://a.com/x"]);
    }
}
