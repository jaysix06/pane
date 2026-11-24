//! Updates from the project's GitHub releases.
//!
//! 1. Check: the latest published (non-draft, non-prerelease) release's tag
//!    is compared with this build's version, a few seconds after launch and
//!    every 6 hours.
//! 2. Download: "Get the update" fetches the release's `Pane-Setup-*.exe`
//!    into `%LOCALAPPDATA%\Pane\updates`, reporting progress, and checks it
//!    against the SHA-256 digest GitHub publishes for the file. A file that
//!    doesn't match is deleted and never run.
//! 3. Install: "Update now" starts the downloaded setup in update mode
//!    (`--quiet --update --relaunch`: it waits for Pane to close, installs
//!    over it, and opens the new version) and closes this window.
//!
//! Everything is published as `shared("update")` with a `phase`:
//! `available`, `downloading`, `ready` or `error`. Requests carry a generic
//! User-Agent and nothing about the user.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use zgx_desktop::{TaskSpawner, WindowHandle};
use zgx_script::{ScriptHost, Shared, Value};

use crate::app::AppRef;

pub const REPO: &str = "jaysix06/pane";
const EVERY: Duration = Duration::from_secs(6 * 3600);

/// This build's version (`PANE_PRETEND_VERSION` overrides it, to try the
/// update flow against a real release).
pub fn current() -> String {
    std::env::var("PANE_PRETEND_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    /// The setup program: where to download it, its name, size and SHA-256.
    pub asset_url: String,
    pub asset_name: String,
    pub asset_size: u64,
    pub sha256: Option<String>,
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

/// The first few lines of release notes as plain text: headings dropped,
/// list markers and emphasis removed.
pub fn plain_notes(markdown: &str) -> String {
    markdown
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let l = l.trim_start_matches(['-', '*', '+']).trim_start();
            l.replace("**", "").replace('`', "").replace("__", "")
        })
        .filter(|l| !l.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Only this project's own release downloads are fetched.
fn trusted_asset_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPO}/releases/download/"))
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
    let notes = plain_notes(&s("body"));
    let asset = v.get("assets").and_then(|a| a.as_array()).and_then(|assets| {
        assets.iter().find(|a| {
            let name = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
            name.starts_with("Pane-Setup-") && name.ends_with(".exe")
        })
    });
    let (asset_url, asset_name, asset_size, sha256) = match asset {
        Some(a) => {
            let url = a.get("browser_download_url").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let name = a.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let size = a.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
            let digest = a
                .get("digest")
                .and_then(|x| x.as_str())
                .and_then(|d| d.strip_prefix("sha256:"))
                .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase);
            if trusted_asset_url(&url) && !name.contains(['/', '\\']) { (url, name, size, digest) } else { Default::default() }
        }
        None => Default::default(),
    };
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        name: s("name"),
        url: s("html_url"),
        notes: notes.chars().take(160).collect(),
        asset_url,
        asset_name,
        asset_size,
        sha256,
    })
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Asks GitHub for the latest release (blocking: call off the UI thread).
/// Ok(None) when the project has no release yet.
pub fn fetch_latest() -> Result<Option<Release>, String> {
    if let Ok(v) = std::env::var("PANE_FAKE_UPDATE") {
        // For trying the sidebar card without publishing a release (no
        // download: it opens the releases page instead).
        return Ok(Some(Release {
            version: v.trim_start_matches('v').to_string(),
            name: format!("Pane {v}"),
            url: format!("https://github.com/{REPO}/releases"),
            notes: "A pretend release for testing the update card.".into(),
            asset_url: String::new(),
            asset_name: String::new(),
            asset_size: 0,
            sha256: None,
        }));
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut response = agent()
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

fn updates_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Pane").join("updates")
}

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A setup already downloaded and verified for `r`, if there is one.
fn verified_download(r: &Release) -> Option<PathBuf> {
    let want = r.sha256.as_ref()?;
    let path = updates_dir().join(&r.asset_name);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() != r.asset_size {
        return None;
    }
    (sha256_file(&path).ok()? == *want).then_some(path)
}

enum DlMsg {
    Progress(u64, u64),
    Done(Result<PathBuf, String>),
}

/// Downloads `r`'s setup, verifying its SHA-256 (blocking, on a thread).
fn download_blocking(r: &Release, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    let want = r.sha256.clone().ok_or("this release has no checksum, so Pane won't install it automatically")?;
    if !trusted_asset_url(&r.asset_url) {
        return Err("unexpected download address".into());
    }
    let dir = updates_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Older downloads are no longer needed.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let target = dir.join(&r.asset_name);
    let part = dir.join(format!("{}.part", r.asset_name));
    let mut response = agent()
        .get(&r.asset_url)
        .header("User-Agent", "Pane-updater")
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 200 {
        return Err(format!("GitHub answered {}", response.status().as_u16()));
    }
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(r.asset_size);
    let mut body = response.body_mut().as_reader();
    let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("the download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(80) {
            last = Instant::now();
            progress(done, total);
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    drop(out);
    progress(done, total);
    let got = hex(&hasher.finalize());
    if got != want {
        let _ = std::fs::remove_file(&part);
        return Err("the download didn't match the release's checksum, so it was deleted".into());
    }
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

#[derive(Default)]
struct UpdateState {
    release: Option<Release>,
    phase: &'static str,
    done: u64,
    total: u64,
    error: Option<String>,
    file: Option<PathBuf>,
}

pub struct Updates {
    out: Shared,
    state: RefCell<UpdateState>,
    tasks: TaskSpawner,
}

/// Starts checking (now, then every 6 hours) and publishes what it finds.
pub fn start(host: &ScriptHost, tasks: &TaskSpawner, app: &AppRef) -> Rc<Updates> {
    let updates = Rc::new(Updates {
        out: host.shared("update", Value::Nil),
        state: RefCell::new(UpdateState::default()),
        tasks: tasks.clone(),
    });
    let (u, a) = (updates.clone(), app.clone());
    tasks.spawn(async move {
        // Let the window settle first.
        zgx::timer::sleep(Duration::from_secs(3)).await;
        loop {
            check(&u, &a);
            zgx::timer::sleep(EVERY).await;
        }
    });
    updates
}

/// Checks now (also the "Check for updates" action).
pub fn check(updates: &Rc<Updates>, app: &AppRef) {
    if !app.borrow().settings.check_updates {
        return;
    }
    // Don't disturb a download in progress or a ready update.
    if matches!(updates.state.borrow().phase, "downloading" | "ready") {
        return;
    }
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let found = fetch_latest().map(|r| {
            r.map(|r| {
                let ready = verified_download(&r);
                (r, ready)
            })
        });
        let _ = tx.send(found);
    });
    let (u, a) = (updates.clone(), app.clone());
    updates.tasks.spawn(async move {
        let Ok(result) = rx.await else { return };
        match result {
            Ok(Some((r, ready))) if is_newer(&r.version, &current()) => {
                let dismissed = a.borrow().settings.dismissed_update.clone();
                let mut s = u.state.borrow_mut();
                if let Some(path) = ready {
                    // Downloaded earlier and still intact: ready to install.
                    s.phase = "ready";
                    s.file = Some(path);
                    s.release = Some(r);
                } else if dismissed != r.version {
                    s.phase = "available";
                    s.release = Some(r);
                } else {
                    *s = UpdateState::default();
                }
                s.error = None;
            }
            Ok(_) => *u.state.borrow_mut() = UpdateState::default(),
            // Offline or rate-limited: try again at the next check.
            Err(e) => eprintln!("[pane] update check: {e}"),
        }
        publish(&u);
    });
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn publish(updates: &Updates) {
    let s = updates.state.borrow();
    updates.out.set(match &s.release {
        Some(r) => Value::record([
            ("version", Value::from(r.version.as_str())),
            ("current", Value::from(current().as_str())),
            ("name", Value::from(r.name.as_str())),
            ("url", Value::from(r.url.as_str())),
            ("notes", Value::from(r.notes.as_str())),
            ("phase", Value::from(s.phase)),
            ("progress", Value::from(if s.total > 0 { s.done as f64 / s.total as f64 } else { 0.0 })),
            ("done_text", Value::from(mb(s.done).as_str())),
            ("total_text", Value::from(mb(if s.total > 0 { s.total } else { r.asset_size }).as_str())),
            ("error", s.error.as_deref().map_or(Value::Nil, Value::from)),
            // Without a downloadable, verifiable setup the button opens the
            // release page instead.
            ("can_download", Value::from(!r.asset_url.is_empty() && r.sha256.is_some())),
        ]),
        None => Value::Nil,
    });
}

/// "Get the update": downloads the setup (or opens the release page when
/// it can't be downloaded and checked).
pub fn download(updates: &Rc<Updates>) {
    let release = {
        let s = updates.state.borrow();
        if s.phase == "downloading" || s.phase == "ready" {
            return;
        }
        s.release.clone()
    };
    let Some(r) = release else { return };
    if r.asset_url.is_empty() || r.sha256.is_none() {
        open_page(&r.url);
        return;
    }
    {
        let mut s = updates.state.borrow_mut();
        s.phase = "downloading";
        s.done = 0;
        s.total = r.asset_size;
        s.error = None;
    }
    publish(updates);

    let (tx, mut rx) = unbounded::<DlMsg>();
    std::thread::Builder::new()
        .name("pane-update-download".into())
        .spawn(move || {
            let ptx = tx.clone();
            let result = download_blocking(&r, &move |done, total| {
                let _ = ptx.unbounded_send(DlMsg::Progress(done, total));
            });
            let _ = tx.unbounded_send(DlMsg::Done(result));
        })
        .expect("download thread");
    let u = updates.clone();
    updates.tasks.spawn(async move {
        while let Some(msg) = rx.next().await {
            {
                let mut s = u.state.borrow_mut();
                match msg {
                    DlMsg::Progress(done, total) => {
                        s.done = done;
                        s.total = total;
                    }
                    DlMsg::Done(Ok(path)) => {
                        s.phase = "ready";
                        s.file = Some(path);
                        s.done = s.total;
                    }
                    DlMsg::Done(Err(e)) => {
                        s.phase = "error";
                        s.error = Some(e);
                    }
                }
            }
            publish(&u);
        }
    });
}

/// "Update now": runs the downloaded setup in update mode and closes Pane;
/// the setup installs over it and opens the new version.
pub fn install(updates: &Rc<Updates>, window: &WindowHandle) {
    let file = {
        let s = updates.state.borrow();
        if s.phase != "ready" {
            return;
        }
        s.file.clone()
    };
    let Some(file) = file.filter(|f| f.is_file()) else {
        updates.state.borrow_mut().phase = "available";
        publish(updates);
        return;
    };
    match std::process::Command::new(&file).args(["--quiet", "--update", "--relaunch"]).spawn() {
        Ok(_) => window.close(),
        Err(e) => {
            let mut s = updates.state.borrow_mut();
            s.phase = "error";
            s.error = Some(format!("the installer couldn't start: {e}"));
            drop(s);
            publish(updates);
        }
    }
}

/// "Later" on the card while an update is only available: hides this
/// version until a newer one comes out.
pub fn dismiss(updates: &Rc<Updates>, app: &AppRef) {
    let version = updates.state.borrow().release.as_ref().map(|r| r.version.clone());
    if let Some(v) = version {
        crate::app::set_setting(app, "dismissed_update", serde_json::json!(v));
    }
    *updates.state.borrow_mut() = UpdateState::default();
    publish(updates);
}

/// After a failed download: back to "available" so it can be tried again.
pub fn reset_error(updates: &Rc<Updates>) {
    let mut s = updates.state.borrow_mut();
    if s.phase == "error" {
        s.phase = "available";
        s.error = None;
    }
    drop(s);
    publish(updates);
}

fn open_page(url: &str) {
    if url.starts_with("https://github.com/") {
        pane_shell::open_path(pane_shell::active_window(), std::path::Path::new(url));
    }
}

/// Opens the release page in the browser ("What's new").
pub fn open(updates: &Rc<Updates>) {
    let url = updates.state.borrow().release.as_ref().map(|r| r.url.clone());
    if let Some(url) = url {
        open_page(&url);
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

    const RELEASE: &str = r#"{"tag_name":"v0.2.0","name":"Pane 0.2.0","html_url":"https://github.com/jaysix06/pane/releases/tag/v0.2.0",
        "draft":false,"prerelease":false,"body":"Thumbnails!\r\n\r\nPreview pane.\r\nFaster grid.",
        "assets":[{"name":"Pane-Setup-0.2.0.exe","size":123,"digest":"sha256:00E8B3FC19549825B2778A7447841C8B0096C61F5198DDDBA4C650D3C3A92916",
        "browser_download_url":"https://github.com/jaysix06/pane/releases/download/v0.2.0/Pane-Setup-0.2.0.exe"}]}"#;

    #[test]
    fn reads_a_github_release() {
        let r = parse_release(RELEASE).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.notes, "Thumbnails! Preview pane.");
        assert_eq!(r.asset_name, "Pane-Setup-0.2.0.exe");
        assert_eq!(r.asset_size, 123);
        assert_eq!(r.sha256.as_deref(), Some("00e8b3fc19549825b2778a7447841c8b0096c61f5198dddba4c650d3c3a92916"));
        let pre = RELEASE.replace("\"prerelease\":false", "\"prerelease\":true");
        assert_eq!(parse_release(&pre), None, "pre-releases aren't offered");
        assert_eq!(parse_release("{}"), None);
    }

    #[test]
    fn only_this_projects_downloads_are_trusted() {
        let elsewhere = RELEASE.replace("https://github.com/jaysix06/pane/releases/download", "https://evil.example/x");
        let r = parse_release(&elsewhere).unwrap();
        assert!(r.asset_url.is_empty() && r.sha256.is_none(), "no download from elsewhere");
        let no_digest = RELEASE.replace("\"digest\":\"sha256:00E8B3FC19549825B2778A7447841C8B0096C61F5198DDDBA4C650D3C3A92916\",", "");
        assert_eq!(parse_release(&no_digest).unwrap().sha256, None);
    }

    #[test]
    fn notes_are_plain_text() {
        let md = "A bug-fix release.\r\n\r\n## Fixed\r\n- **Hovering a small category** darkened the bar.\r\n- `Other` fix.";
        assert_eq!(plain_notes(md), "A bug-fix release. Hovering a small category darkened the bar.");
    }

    #[test]
    fn hashes_files() {
        let p = std::env::temp_dir().join(format!("pane-hash-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_file(p).unwrap();
    }
}
