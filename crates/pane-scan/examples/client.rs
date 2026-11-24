//! Talks to the helper service directly: `cargo run -p pane-scan --example client -- C:`
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::AtomicBool;

fn main() {
    let volume = std::env::args().nth(1).unwrap_or_else(|| "C:".into());
    println!("installed: {}", pane_scan::client::installed());
    let raw = std::env::args().any(|a| a == "--raw");
    if raw {
        // Send a raw line and print what comes back (protocol checks).
        let line = std::env::args().nth(3).unwrap_or_else(|| format!("{{\"scan\":\"{volume}\"}}"));
        let mut f = std::fs::OpenOptions::new().read(true).write(true).open(pane_scan::ipc::PIPE).expect("open pipe");
        f.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut r = BufReader::new(f);
        let mut s = String::new();
        r.read_line(&mut s).unwrap();
        println!("reply: {}", &s[..s.len().min(200)]);
        return;
    }
    let started = std::time::Instant::now();
    let report = |e: pane_scan::Event| match e {
        pane_scan::Event::Progress(p) => eprint!("\r{:>4}‰ {}            ", p.permille, p.files),
        pane_scan::Event::Done(r) => println!("\ndone: {:?} {} files, took {} ms (service), {:.1} s here", r.mode, r.files, r.took_ms, started.elapsed().as_secs_f64()),
        pane_scan::Event::Error(e) => println!("\nerror: {e}"),
    };
    let ok = pane_scan::client::scan(&volume, &report, &AtomicBool::new(false));
    println!("client::scan returned {ok}");
}
