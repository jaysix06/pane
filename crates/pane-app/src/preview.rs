//! The preview pane: what the selected item looks like and its details,
//! published as the shared "preview" value (ui/components/preview.luau).
//!
//! Details the listing already has show at once; the rest (a picture, the
//! start of a text file, when it was created, what a folder holds) come from
//! one background worker. It only ever does the newest request: moving
//! through a folder with the arrow keys skips the items passed over.
//!
//! Content is one of:
//! - "image": the shell's thumbnail, large (pictures, videos, and any type
//!   with a thumbnail handler, such as PDFs once a reader installs one);
//! - "text": the start of a text or code file, up to the size limit;
//! - "icon": the file's icon, for everything else.

use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::Read;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;
use zgx_script::{ScriptHost, Shared, Value};

/// The side previews are made at: sharp across the pane on a 200% display.
const PIXELS: u32 = 768;
/// Folders are counted up to this many items.
const COUNT_LIMIT: usize = 100_000;

/// What to preview, from the selection.
pub enum Target {
    Nothing,
    Many(usize),
    Item(Item),
}

/// The selected item, with what the listing already knows of it.
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    pub dir: bool,
    /// The listing's texts: "JPG File", "1.2 MB", "11/26/2026 7:40 AM".
    pub kind: String,
    pub size: String,
    pub date: String,
    /// The icon key (icons.rs), for the large icon shown meanwhile.
    pub icon: String,
    /// A picture or video (thumbs.rs has a thumbnail name for it).
    pub picture: bool,
    pub modified: u64,
}

struct Job {
    generation: u64,
    path: PathBuf,
    dir: bool,
    picture: bool,
    max_bytes: usize,
}

enum Content {
    Image(pane_shell::Icon),
    Text { text: String, truncated: bool },
    Icon,
}

struct Done {
    generation: u64,
    created: Option<u64>,
    /// A folder's item count, and whether it stopped counting.
    items: Option<(usize, bool)>,
    content: Content,
}

struct Inner {
    host: ScriptHost,
    out: Shared,
    slot: Arc<(Mutex<Option<Job>>, Condvar)>,
    generation: Cell<u64>,
    /// What is shown: path, modified time and size limit, so a selection
    /// published again (a click on the same row) loads nothing new.
    shown: RefCell<Option<String>>,
    /// The item shown, kept to add the worker's findings to.
    item: RefCell<Option<Item>>,
    /// The host image showing now, cleared when another replaces it.
    image: RefCell<Option<String>>,
}

#[derive(Clone)]
pub struct Preview(Rc<Inner>);

impl Preview {
    /// Starts the worker. Spawn the returned future on the window's tasks.
    pub fn start(host: ScriptHost) -> (Preview, impl Future<Output = ()>) {
        let slot = Arc::new((Mutex::new(None::<Job>), Condvar::new()));
        let (done_tx, done_rx) = unbounded::<Done>();
        let worker_slot = slot.clone();
        std::thread::Builder::new()
            .name("pane-preview".into())
            .spawn(move || work(&worker_slot, &done_tx))
            .expect("preview thread");
        let preview = Preview(Rc::new(Inner {
            host: host.clone(),
            out: host.shared("preview", Value::Nil),
            slot,
            generation: Cell::new(0),
            shown: RefCell::new(None),
            item: RefCell::new(None),
            image: RefCell::new(None),
        }));
        let finish = finish(preview.clone(), done_rx);
        (preview, finish)
    }

    /// Shows `target`; `max_kb` caps how much of a text file is read.
    pub fn show(&self, target: Target, max_kb: u32) {
        let p = &self.0;
        let key = match &target {
            Target::Nothing => "-".to_string(),
            Target::Many(n) => format!("*{n}"),
            Target::Item(item) => format!("{}|{}|{max_kb}", item.path.display(), item.modified),
        };
        if p.shown.borrow().as_deref() == Some(key.as_str()) {
            return;
        }
        *p.shown.borrow_mut() = Some(key);
        let generation = p.generation.get() + 1;
        p.generation.set(generation);
        self.drop_image();
        match target {
            Target::Nothing => {
                *p.item.borrow_mut() = None;
                p.out.set(Value::record([("state", Value::from("empty"))]));
            }
            Target::Many(n) => {
                *p.item.borrow_mut() = None;
                p.out.set(Value::record([("state", Value::from("many")), ("count", Value::from(n as f64))]));
            }
            Target::Item(item) => {
                let job = Job {
                    generation,
                    path: item.path.clone(),
                    dir: item.dir,
                    picture: item.picture,
                    max_bytes: max_kb.max(1) as usize * 1024,
                };
                p.out.set(item_value(&item, None, Value::from("loading"), &[]));
                *p.item.borrow_mut() = Some(item);
                let (lock, wake) = &*p.slot;
                *lock.lock().expect("preview slot") = Some(job);
                wake.notify_one();
            }
        }
    }

    /// The pane is closed: show nothing and free the picture.
    pub fn clear(&self) {
        let p = &self.0;
        if p.shown.borrow().is_none() {
            return;
        }
        *p.shown.borrow_mut() = None;
        *p.item.borrow_mut() = None;
        p.generation.set(p.generation.get() + 1);
        *p.slot.0.lock().expect("preview slot") = None;
        self.drop_image();
        p.out.set(Value::Nil);
    }

    fn drop_image(&self) {
        if let Some(name) = self.0.image.borrow_mut().take() {
            self.0.host.image(&name, None);
        }
    }
}

fn item_value(item: &Item, src: Option<&str>, content: Value, extra: &[(&str, Value)]) -> Value {
    let mut fields: Vec<(&str, Value)> = vec![
        ("state", Value::from("item")),
        ("name", Value::from(item.name.as_str())),
        ("dir", Value::from(item.dir)),
        ("icon", Value::from(item.icon.as_str())),
        ("path", Value::from(item.path.display().to_string().as_str())),
        ("content", content),
        ("image", Value::from(src.unwrap_or(""))),
    ];
    fields.extend(extra.iter().cloned());
    Value::record(fields)
}

fn details(item: &Item, created: Option<u64>, items: Option<(usize, bool)>) -> Value {
    let mut rows: Vec<(&str, String)> = vec![("Type", item.kind.clone())];
    if let Some((n, more)) = items {
        let plus = if more { "+" } else { "" };
        rows.push(("Contains", if n == 1 { "1 item".into() } else { format!("{n}{plus} items") }));
    }
    if !item.size.is_empty() {
        rows.push(("Size", item.size.clone()));
    }
    if !item.date.is_empty() {
        rows.push(("Modified", item.date.clone()));
    }
    if let Some(c) = created.filter(|c| *c > 0) {
        rows.push(("Created", pane_fs::format_modified(c)));
    }
    if let Some(parent) = item.path.parent() {
        rows.push(("Location", parent.display().to_string()));
    }
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(label, value)| Value::record([("label", Value::from(label)), ("value", Value::from(value.as_str()))]))
        .collect();
    Value::List(Rc::from(list))
}

async fn finish(preview: Preview, mut done: UnboundedReceiver<Done>) {
    while let Some(d) = done.next().await {
        let p = &preview.0;
        if d.generation != p.generation.get() {
            continue;
        }
        let item = p.item.borrow();
        let Some(item) = item.as_ref() else { continue };
        let details = details(item, d.created, d.items);
        let value = match d.content {
            Content::Image(icon) => match zgx::image::ImageData::new(icon.width, icon.height, icon.rgba) {
                Ok(image) => {
                    let name = format!("p/{}", d.generation);
                    p.host.image(&name, Some(Arc::new(image)));
                    *p.image.borrow_mut() = Some(name.clone());
                    item_value(item, Some(&format!("host:{name}")), Value::from("image"), &[("details", details)])
                }
                Err(_) => item_value(item, None, Value::from("icon"), &[("details", details)]),
            },
            Content::Text { text, truncated } => item_value(
                item,
                None,
                Value::from("text"),
                &[("details", details), ("text", Value::from(text.as_str())), ("truncated", Value::from(truncated))],
            ),
            Content::Icon => item_value(item, None, Value::from("icon"), &[("details", details)]),
        };
        p.out.set(value);
    }
}

fn work(slot: &(Mutex<Option<Job>>, Condvar), done: &UnboundedSender<Done>) {
    pane_shell::init_com();
    let (lock, wake) = slot;
    loop {
        let job = {
            let mut s = lock.lock().expect("preview slot");
            loop {
                if let Some(job) = s.take() {
                    break job;
                }
                s = wake.wait(s).expect("preview slot");
            }
        };
        let created = std::fs::metadata(&job.path).ok().map(|m| m.creation_time());
        let (items, content) = if job.dir {
            (Some(count_items(&job.path)), Content::Icon)
        } else if job.picture {
            (None, pane_shell::thumbnail_rgba(&job.path, PIXELS).map_or(Content::Icon, Content::Image))
        } else {
            let content = match read_text(&job.path, job.max_bytes) {
                Some((text, truncated)) => Content::Text { text, truncated },
                None => pane_shell::thumbnail_rgba(&job.path, PIXELS).map_or(Content::Icon, Content::Image),
            };
            (None, content)
        };
        if done.unbounded_send(Done { generation: job.generation, created, items, content }).is_err() {
            return;
        }
    }
}

fn count_items(dir: &std::path::Path) -> (usize, bool) {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let n = entries.take(COUNT_LIMIT + 1).count();
            (n.min(COUNT_LIMIT), n > COUNT_LIMIT)
        }
        Err(_) => (0, false),
    }
}

/// The start of `path` as text, and whether there was more, or `None` for a
/// file that isn't text.
fn read_text(path: &std::path::Path, max_bytes: usize) -> Option<(String, bool)> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1).read_to_end(&mut bytes).ok()?;
    let truncated = bytes.len() > max_bytes;
    bytes.truncate(max_bytes);
    decode_text(&bytes).map(|text| (text, truncated))
}

/// Text from bytes: UTF-16 or UTF-8 by their byte order marks, else UTF-8,
/// else Windows' 8-bit text (read as Latin-1). `None` for binary data.
fn decode_text(bytes: &[u8]) -> Option<String> {
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Some(utf16(rest, true));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Some(utf16(rest, false));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_string()),
        // Cut in the middle of a character at the size limit: fine.
        Err(e) if e.error_len().is_none() => Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned()),
        Err(_) => {
            let control = head.iter().filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C)).count();
            (control * 100 <= head.len()).then(|| bytes.iter().map(|&b| b as char).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_text_and_refuses_binary() {
        assert_eq!(decode_text(b"hello\nworld").as_deref(), Some("hello\nworld"));
        assert_eq!(decode_text(b"\xEF\xBB\xBFbom").as_deref(), Some("bom"));
        assert_eq!(decode_text(b"\xFF\xFEh\0i\0").as_deref(), Some("hi"));
        assert_eq!(decode_text(b"caf\xC3").as_deref(), Some("caf"), "a character cut at the limit");
        assert_eq!(decode_text(b"caf\xE9 au lait").as_deref(), Some("café au lait"), "8-bit text");
        assert_eq!(decode_text(b"MZ\x90\0\x03\0\0\0"), None, "a program");
        assert_eq!(decode_text(b"\x01\x02\x03\x04\x05\xFF\xFE\x80").map(|_| ()), None, "noise");
    }

    #[test]
    fn reads_up_to_the_limit() {
        let file = std::env::temp_dir().join(format!("pane-preview-test-{}.txt", std::process::id()));
        std::fs::write(&file, "abcdef").unwrap();
        assert_eq!(read_text(&file, 4), Some(("abcd".into(), true)));
        assert_eq!(read_text(&file, 6), Some(("abcdef".into(), false)));
        std::fs::remove_file(&file).unwrap();
    }
}
