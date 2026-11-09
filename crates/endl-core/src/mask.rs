//! Secret masking for log lines and diagnostics. Cookies, tokens, passwords, auth headers
//! and signed-URL parameters never reach the log file in clear text.

const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "auth",
    "authorization",
    "password",
    "passwd",
    "pass",
    "secret",
    "rpc-secret",
    "api_key",
    "apikey",
    "key",
    "sig",
    "signature",
    "session",
    "sessionid",
    "sid",
    "cookie",
    "cookies",
    "x-amz-signature",
    "x-amz-credential",
    "x-amz-security-token",
];

const MASK: &str = "***";

fn is_sensitive(key: &str) -> bool {
    let k = key.trim().trim_start_matches('-').to_ascii_lowercase();
    SENSITIVE_KEYS.contains(&k.as_str())
}

/// Mask secrets in a single line of text.
pub fn mask_secrets(line: &str) -> String {
    let mut out = mask_headers(line);
    out = mask_pairs(&out, '=');
    out = mask_pairs(&out, ':');
    out
}

/// `Cookie: …`, `Authorization: Bearer …`, `Set-Cookie: …` → value masked to end of line.
fn mask_headers(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for h in ["authorization:", "proxy-authorization:", "set-cookie:", "cookie:"] {
        if let Some(i) = lower.find(h) {
            // Only treat it as a header when it starts the line or follows a separator.
            let ok = i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric();
            if ok {
                return format!("{}{} {MASK}", &line[..i], &line[i..i + h.len()]);
            }
        }
    }
    line.to_string()
}

/// `key=value` / `"key": "value"` / `--rpc-secret value`-style pairs where key is sensitive.
fn mask_pairs(line: &str, sep: char) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == sep {
            // Walk back over the key (letters, digits, -, _ and an optional closing quote).
            let mut k_end = out.len();
            if out.ends_with('"') {
                k_end -= 1;
            }
            let key_start = out[..k_end]
                .char_indices()
                .rev()
                .find(|&(_, c)| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .map_or(0, |(p, c)| p + c.len_utf8());
            let key = &out[key_start..k_end];
            if !key.is_empty() && is_sensitive(key) {
                out.push(sep);
                i += 1;
                // Keep whitespace and an opening quote, then skip the value.
                while i < chars.len() && chars[i] == ' ' {
                    out.push(' ');
                    i += 1;
                }
                let quoted = i < chars.len() && chars[i] == '"';
                if quoted {
                    out.push('"');
                    i += 1;
                }
                let start = i;
                while i < chars.len() {
                    let c = chars[i];
                    let end = if quoted { c == '"' } else { c == '&' || c == ' ' || c == ',' || c == ';' || c == '\'' || c == '}' };
                    if end {
                        break;
                    }
                    i += 1;
                }
                if i > start {
                    out.push_str(MASK);
                }
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `--cookies /path`, `--rpc-secret X`, `--add-header "Cookie: …"` in a command line.
pub fn mask_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for a in args {
        if hide_next {
            out.push(MASK.to_string());
            hide_next = false;
            continue;
        }
        let flag = a.split('=').next().unwrap_or("");
        if matches!(flag, "--rpc-secret" | "--password" | "--http-passwd" | "--ftp-passwd" | "--cookies" | "--load-cookies" | "--username" | "-p" | "-u") {
            if a.contains('=') {
                out.push(format!("{flag}={MASK}"));
            } else {
                out.push(a.clone());
                hide_next = true;
            }
            continue;
        }
        out.push(mask_secrets(a));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers() {
        assert_eq!(mask_secrets("Cookie: SID=abc; HSID=def"), "Cookie: ***");
        assert_eq!(mask_secrets("sending Authorization: Bearer xyz"), "sending Authorization: ***");
    }

    #[test]
    fn query_parameters() {
        let s = mask_secrets("GET https://cdn.x.com/v.mp4?id=5&token=SECRET123&sig=abc&x=1");
        assert_eq!(s, "GET https://cdn.x.com/v.mp4?id=5&token=***&sig=***&x=1");
    }

    #[test]
    fn json_and_cli() {
        assert_eq!(mask_secrets(r#"{"secret": "hunter2", "name": "a"}"#), r#"{"secret": "***", "name": "a"}"#);
        assert_eq!(mask_secrets("aria2c --rpc-secret=abcdef --max=3"), "aria2c --rpc-secret=*** --max=3");
    }

    #[test]
    fn harmless_text_unchanged() {
        let s = "Downloaded 10 MB at 3 MB/s: ubuntu.iso (time=12s)";
        assert_eq!(mask_secrets(s), s);
        assert_eq!(mask_secrets("monkey=banana"), "monkey=banana");
    }

    #[test]
    fn args() {
        let a: Vec<String> = ["yt-dlp", "--cookies", "/home/me/c.txt", "--rpc-secret=zz", "https://x.com/?token=1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(mask_args(&a), vec!["yt-dlp", "--cookies", "***", "--rpc-secret=***", "https://x.com/?token=***"]);
    }
}
