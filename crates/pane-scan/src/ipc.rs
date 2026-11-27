//! The helper service's protocol, shared by the service and Pane.
//!
//! Pane connects to `PIPE` and writes one JSON line, then reads JSON lines
//! until the last:
//! - `{"scan":"C:"}`: `Event`s until `done` or `error` (the Overview);
//! - `{"find":{...}}`: `find::FindReply`s until `done` or `error` (a search
//!   of the whole drive, see `find::FindRequest`).
//!
//! Those are the only requests.

use serde::{Deserialize, Serialize};

use crate::find::FindRequest;

pub const SERVICE: &str = "PaneScan";
pub const SERVICE_DISPLAY: &str = "Pane instant scan";
pub const PIPE: &str = r"\\.\pipe\pane-scan";
/// Longest request line accepted.
pub const MAX_REQUEST: usize = 1024;

/// A request as written on the pipe: exactly one of the two.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find: Option<FindRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Scan(String),
    Find(FindRequest),
}

impl Request {
    /// Parses and validates a request line: a scan of a drive letter and
    /// colon, or a find (see `FindRequest::valid`).
    pub fn parse(line: &str) -> Result<Request, String> {
        let w: Wire = serde_json::from_str(line.trim()).map_err(|_| "bad request".to_string())?;
        match (w.scan, w.find) {
            (Some(scan), None) => {
                let b = scan.as_bytes();
                if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
                    Ok(Request::Scan(scan.to_ascii_uppercase()))
                } else {
                    Err("bad volume".into())
                }
            }
            (None, Some(mut find)) => {
                find.valid()?;
                find.volume = find.volume.to_ascii_uppercase();
                Ok(Request::Find(find))
            }
            _ => Err("bad request".into()),
        }
    }

    /// The line to write for it.
    pub fn line(&self) -> String {
        let w = match self {
            Request::Scan(v) => Wire { scan: Some(v.clone()), find: None },
            Request::Find(f) => Wire { scan: None, find: Some(f.clone()) },
        };
        serde_json::to_string(&w).unwrap_or_default() + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_drive_letters_pass() {
        assert_eq!(Request::parse(r#"{"scan":"c:"}"#).unwrap(), Request::Scan("C:".into()));
        assert!(Request::parse(r#"{"scan":"C:\\Windows"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"\\\\.\\PhysicalDrive0"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"C:\\"}"#).is_err());
        assert!(Request::parse(r#"{"scan":"C:","run":"cmd"}"#).is_err(), "unknown fields refused");
        assert!(Request::parse("not json").is_err());
    }

    #[test]
    fn finds_round_trip_and_are_checked() {
        let find = FindRequest {
            volume: "d:".into(),
            query: "*.rs".into(),
            case_sensitive: false,
            whole_name: false,
            hidden: false,
            max_depth: 8,
            limit: 500,
        };
        let line = Request::Find(find.clone()).line();
        assert!(line.ends_with('\n'));
        let back = Request::parse(&line).unwrap();
        assert_eq!(back, Request::Find(FindRequest { volume: "D:".into(), ..find.clone() }));
        assert!(Request::parse(r#"{"scan":"C:","find":null}"#).is_ok());
        let both = format!(r#"{{"scan":"C:","find":{}}}"#, serde_json::to_string(&find).unwrap());
        assert!(Request::parse(&both).is_err(), "one request at a time");
        assert!(Request::parse("{}").is_err());
    }
}

#[cfg(test)]
mod name_tests {
    #[test]
    fn pipe_name_is_a_local_pipe_path() {
        assert!(super::PIPE.starts_with(r"\\.\pipe\"), "{}", super::PIPE);
    }
}
