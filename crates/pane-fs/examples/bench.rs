//! Times each stage of loading a folder: `cargo run --release -p pane-fs --example bench -- <dir>`
use std::time::Instant;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench <dir>");
    let t = Instant::now();
    let mut entries = pane_fs::list_dir(dir.as_ref()).unwrap();
    let list = t.elapsed();
    let t = Instant::now();
    pane_fs::sort_entries(&mut entries, pane_fs::SortKey::Name, false);
    let sort = t.elapsed();
    let t = Instant::now();
    let dates: Vec<String> = entries.iter().map(|e| pane_fs::format_modified(e.modified)).collect();
    let date = t.elapsed();
    let t = Instant::now();
    let sizes: Vec<String> = entries.iter().map(|e| pane_fs::format_size(e.size)).collect();
    let size = t.elapsed();
    println!(
        "{} entries: list {:.1} ms, sort {:.1} ms, dates {:.1} ms, sizes {:.1} ms",
        entries.len(),
        list.as_secs_f64() * 1e3,
        sort.as_secs_f64() * 1e3,
        date.as_secs_f64() * 1e3,
        size.as_secs_f64() * 1e3
    );
    assert_eq!(dates.len() + sizes.len(), entries.len() * 2);
}
