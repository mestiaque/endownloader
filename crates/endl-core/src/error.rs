//! Typed errors. Every failure the user can see maps to one of these, and each has a
//! short, plain message (`Display`). The UI also gets the `kind` so it can pick an icon or
//! offer an action (e.g. "Add cookies" for `LoginRequired`).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DlError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("The file was not found on the server (404)")]
    NotFound,
    #[error("The server refused access (403)")]
    Forbidden,
    #[error("The site is limiting requests (429). Try again later")]
    RateLimited,
    #[error("This content needs a login. Add cookies for this site in Settings")]
    LoginRequired,
    #[error("This content was removed or is private")]
    Removed,
    #[error("This content is not available in your region")]
    GeoBlocked,
    #[error("This content is DRM-protected and cannot be downloaded")]
    Drm,
    #[error("Not enough disk space")]
    DiskFull,
    #[error("{0} is missing. Open Settings → Tools to install it")]
    ToolMissing(String),
    #[error("{0} is outdated. Open Settings → Tools to update it")]
    ToolOutdated(String),
    #[error("Not a valid link: {0}")]
    InvalidUrl(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Database error: {0}")]
    Db(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Other(String),
}

impl DlError {
    pub fn kind(&self) -> &'static str {
        match self {
            DlError::Network(_) => "network",
            DlError::NotFound => "notFound",
            DlError::Forbidden => "forbidden",
            DlError::RateLimited => "rateLimited",
            DlError::LoginRequired => "loginRequired",
            DlError::Removed => "removed",
            DlError::GeoBlocked => "geoBlocked",
            DlError::Drm => "drm",
            DlError::DiskFull => "diskFull",
            DlError::ToolMissing(_) => "toolMissing",
            DlError::ToolOutdated(_) => "toolOutdated",
            DlError::InvalidUrl(_) => "invalidUrl",
            DlError::Cancelled => "cancelled",
            DlError::Db(_) => "db",
            DlError::Io(_) => "io",
            DlError::Other(_) => "other",
        }
    }

    /// Whether automatic retry with backoff makes sense.
    pub fn is_transient(&self) -> bool {
        matches!(self, DlError::Network(_) | DlError::RateLimited)
    }

    /// Map an HTTP status code to an error (None for success codes).
    pub fn from_http_status(code: u16) -> Option<DlError> {
        match code {
            200..=399 => None,
            401 => Some(DlError::LoginRequired),
            403 => Some(DlError::Forbidden),
            404 | 410 => Some(DlError::NotFound),
            429 => Some(DlError::RateLimited),
            451 => Some(DlError::GeoBlocked),
            c => Some(DlError::Network(format!("server answered HTTP {c}"))),
        }
    }
}

impl From<rusqlite::Error> for DlError {
    fn from(e: rusqlite::Error) -> Self {
        DlError::Db(e.to_string())
    }
}

impl From<std::io::Error> for DlError {
    fn from(e: std::io::Error) -> Self {
        // ENOSPC (28) on Unix, ERROR_DISK_FULL (112) / ERROR_HANDLE_DISK_FULL (39) on Windows.
        match e.raw_os_error() {
            Some(28) if cfg!(unix) => DlError::DiskFull,
            Some(112) | Some(39) if cfg!(windows) => DlError::DiskFull,
            _ => DlError::Io(e.to_string()),
        }
    }
}

impl From<serde_json::Error> for DlError {
    fn from(e: serde_json::Error) -> Self {
        DlError::Other(format!("Invalid data: {e}"))
    }
}

/// Sent to the frontend as `{ kind, message }`.
impl Serialize for DlError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("DlError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T, E = DlError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_mapping() {
        assert_eq!(DlError::from_http_status(200), None);
        assert_eq!(DlError::from_http_status(206), None);
        assert_eq!(DlError::from_http_status(404), Some(DlError::NotFound));
        assert_eq!(DlError::from_http_status(403), Some(DlError::Forbidden));
        assert_eq!(DlError::from_http_status(429), Some(DlError::RateLimited));
        assert_eq!(DlError::from_http_status(401), Some(DlError::LoginRequired));
        assert!(matches!(DlError::from_http_status(503), Some(DlError::Network(_))));
    }

    #[test]
    fn serializes_kind_and_message() {
        let v = serde_json::to_value(DlError::RateLimited).unwrap();
        assert_eq!(v["kind"], "rateLimited");
        assert!(v["message"].as_str().unwrap().contains("429"));
    }

    #[test]
    fn transient() {
        assert!(DlError::Network("x".into()).is_transient());
        assert!(!DlError::NotFound.is_transient());
    }
}
