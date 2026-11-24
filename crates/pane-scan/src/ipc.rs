//! The helper service's protocol, shared by the service and Pane.
//!
//! Pane connects to `PIPE`, writes one JSON line `{"scan":"C:"}`, and reads
//! JSON lines of `Event` until `done` or `error`. That is the only request.

use serde::{Deserialize, Serialize};

pub const SERVICE: &str = "PaneScan";
pub const SERVICE_DISPLAY: &str = "Pane instant scan";
pub const PIPE: &str = r"\\.\pipe\pane-scan";
/// Longest request line accepted.
pub const MAX_REQUEST: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub scan: String,
}

impl Request {
    /// Parses and validates a request line: only a drive letter and colon.
    pub fn parse(line: &str) -> Result<Request, String> {
        let r: Request = serde_json::from_str(line.trim()).map_err(|_| "bad request".to_string())?;
        let b = r.scan.as_bytes();
        if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            Ok(Request { scan: r.scan.to_ascii_uppercase() })
        } else {
            Err("bad volume".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_drive_letters_pass() {
        assert_eq!(Request::parse(r#"{"scan":"c:"}"#).unwrap().scan, "C:");
        assert!(Request::parse(r#"{"scan":"C:\\Windows"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"\\\\.\\PhysicalDrive0"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"C:\\"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"C:","run":"cmd"}"#).is_err(), "unknown fields refused");
        assert!(Request::parse("not json").is_err());
    }
}

#[cfg(test)]
mod name_tests {
    #[test]
    fn pipe_name_is_a_local_pipe_path() {
        assert!(super::PIPE.starts_with(r"\\.\pipe\"), "{}", super::PIPE);
    }
}
