//! Update checks against the project's GitHub releases: the latest
//! published (non-draft, non-prerelease) release's tag is compared with
//! this build's version. A newer one is published as `shared("update")`,
//! which the sidebar shows; "Later" remembers the version so it stays quiet
//! until the next one.
//!
//! The request goes to api.github.com with a generic User-Agent and nothing
//! about the user.

use std::rc::Rc;
use std::time::Duration;

use zgx_desktop::TaskSpawner;
use zgx_script::{ScriptHost, Shared, Value};

use crate::app::AppRef;

pub const REPO: &str = "jaysix06/pane";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    pub published: String,
}

/// `major.minor.patch` from a tag like "v1.2.3" or "1.2" (missing parts are
/// 0; a pre-release or build suffix is ignored).
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    let core = t.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>());
    let major = parts.next()?.ok()?;
    let minor = parts.next().unwrap_or(Ok(0)).ok()?;
    let patch = parts.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

/// Whether `latest` is newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Reads GitHub's "latest release" answer.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("draft").and_then(|d| d.as_bool()) == Some(true) || v.get("prerelease").and_then(|d| d.as_bool()) == Some(true) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?.to_string();
    parse_version(&tag)?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let notes: String = s("body").lines().map(str::trim).filter(|l| !l.is_empty()).take(3).collect::<Vec<_>>().join(" ");
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        name: s("name"),
        url: s("html_url"),
        notes: notes.chars().take(160).collect(),
        published: s("published_at"),
    })
}

/// Asks GitHub for the latest release (blocking: call off the UI thread).
/// Ok(None) when the project has no release yet.
pub fn fetch_latest() -> Result<Option<Release>, String> {
    if let Ok(v) = std::env::var("PANE_FAKE_UPDATE") {
        // For trying the sidebar card without publishing a release.
        return Ok(Some(Release {
            version: v.trim_start_matches('v').to_string(),
            name: format!("Pane {v}"),
            url: format!("https://github.com/{REPO}/releases"),
            notes: "A pretend release for testing the update card.".into(),
            published: String::new(),
        }));
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("User-Agent", "Pane-updater")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    match response.status().as_u16() {
        404 => Ok(None),
        200 => {
            let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
            Ok(parse_release(&body))
        }
        code => Err(format!("GitHub answered {code}")),
    }
}

pub struct Updates {
    out: Shared,
}

/// Starts checking (now, then every 6 hours) and publishes what it finds.
pub fn start(host: &ScriptHost, tasks: &TaskSpawner, app: &AppRef) -> Rc<Updates> {
    let updates = Rc::new(Updates { out: host.shared("update", Value::Nil) });
    let (u, a, t) = (updates.clone(), app.clone(), tasks.clone());
    tasks.spawn(async move {
        // Let the window settle first.
        zgx::timer::sleep(Duration::from_secs(3)).await;
        loop {
            check(&u, &a, &t);
            zgx::timer::sleep(EVERY).await;
        }
    });
    updates
}

/// Checks now (also the "Check for updates" action).
pub fn check(updates: &Rc<Updates>, app: &AppRef, tasks: &TaskSpawner) {
    if !app.borrow().settings.check_updates {
        return;
    }
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(fetch_latest());
    });
    let (u, a) = (updates.clone(), app.clone());
    tasks.spawn(async move {
        let Ok(result) = rx.await else { return };
        match result {
            Ok(Some(r)) if is_newer(&r.version, CURRENT) => {
                let dismissed = a.borrow().settings.dismissed_update.clone();
                if dismissed != r.version {
                    publish(&u, Some(&r));
                    return;
                }
                publish(&u, None);
            }
            Ok(_) => publish(&u, None),
            // Offline or rate-limited: try again at the next check.
            Err(e) => eprintln!("[pane] update check: {e}"),
        }
    });
}

fn publish(updates: &Updates, r: Option<&Release>) {
    updates.out.set(match r {
        Some(r) => Value::record([
            ("version", Value::from(r.version.as_str())),
            ("current", Value::from(CURRENT)),
            ("name", Value::from(r.name.as_str())),
            ("url", Value::from(r.url.as_str())),
            ("notes", Value::from(r.notes.as_str())),
        ]),
        None => Value::Nil,
    });
}

/// "Later": hides this version's card until a newer one comes out.
pub fn dismiss(updates: &Rc<Updates>, app: &AppRef) {
    let version = updates.out.get().field("version").and_then(|v| match v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    });
    if let Some(v) = version {
        crate::app::set_setting(app, "dismissed_update", serde_json::json!(v));
    }
    publish(updates, None);
}

/// Opens the release page in the browser.
pub fn open(updates: &Rc<Updates>) {
    let url = updates.out.get().field("url").and_then(|v| match v {
        Value::Str(s) if s.starts_with("https://github.com/") => Some(s.to_string()),
        _ => None,
    });
    if let Some(url) = url {
        pane_shell::open_path(pane_shell::active_window(), std::path::Path::new(&url));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.2"), Some((0, 2, 0)));
        assert_eq!(parse_version("v2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("nightly"), None);
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"), "numeric, not text");
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn reads_a_github_release() {
        let json = r#"{"tag_name":"v0.2.0","name":"Pane 0.2.0","html_url":"https://github.com/jaysix06/pane/releases/tag/v0.2.0",
            "draft":false,"prerelease":false,"published_at":"2026-12-01T10:00:00Z","body":"Thumbnails!\r\n\r\nPreview pane.\r\nFaster grid."}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.url, "https://github.com/jaysix06/pane/releases/tag/v0.2.0");
        assert_eq!(r.notes, "Thumbnails! Preview pane. Faster grid.");
        let pre = json.replace("\"prerelease\":false", "\"prerelease\":true");
        assert_eq!(parse_release(&pre), None, "pre-releases aren't offered");
        assert_eq!(parse_release("{}"), None);
    }
}
