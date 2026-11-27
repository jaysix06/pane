//! Times the shell's thumbnails for the pictures in a folder: one pass,
//! then a second (Windows' own cache warm). `cargo run -p pane-shell
//! --release --example thumbtime -- <folder>`
use std::time::Instant;

fn main() {
    pane_shell::init_com();
    let dir = std::env::args().nth(1).expect("folder");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                ["jpg", "jpeg", "png", "webp", "mp4", "gif"].contains(&e.to_ascii_lowercase().as_str())
            })
        })
        .collect();
    for pass in 1..=3 {
        let t = Instant::now();
        let mut ok = 0;
        for f in &files {
            if pane_shell::thumbnail_rgba(f, 256).is_some() {
                ok += 1;
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!("pass {pass}: {ok}/{} thumbnails, {ms:.1} ms, {:.2} ms each", files.len(), ms / files.len().max(1) as f64);
    }
}
