//! Sends one find to the helper service's pipe and prints what comes back,
//! without starting the service (for `pane-scan-service debug-serve`):
//! `cargo run -p pane-scan --example find -- C: report`
use std::io::{BufRead, BufReader, Write};

use pane_scan::find::{FindReply, FindRequest};
use pane_scan::ipc::{PIPE, Request};

fn main() {
    let mut args = std::env::args().skip(1);
    let volume = args.next().unwrap_or_else(|| "C:".into());
    let query = args.next().unwrap_or_else(|| "readme".into());
    let request = Request::Find(FindRequest {
        volume,
        query,
        case_sensitive: false,
        whole_name: false,
        hidden: false,
        max_depth: 64,
        limit: 2000,
    });
    for round in 1..=2 {
        let started = std::time::Instant::now();
        let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE).expect("open pipe");
        pipe.write_all(request.line().as_bytes()).unwrap();
        let mut lines = BufReader::new(pipe).lines();
        let mut shown = 0;
        while let Some(Ok(line)) = lines.next() {
            match serde_json::from_str::<FindReply>(&line) {
                Ok(FindReply::Found(batch)) => {
                    for f in batch.iter().take(5usize.saturating_sub(shown)) {
                        println!("  {}{}", f.path, if f.dir { "\\" } else { "" });
                    }
                    shown += batch.len();
                }
                Ok(FindReply::Done { matched, took_ms, fresh }) => {
                    println!(
                        "round {round}: {matched} matches, {took_ms} ms in the service (fresh table: {fresh}), {:.0} ms here",
                        started.elapsed().as_secs_f64() * 1e3
                    );
                    break;
                }
                Ok(FindReply::Error(e)) => {
                    println!("error: {e}");
                    break;
                }
                Err(e) => {
                    println!("bad line ({e}): {line}");
                    break;
                }
            }
        }
        // debug-serve makes its pipe again between clients.
        std::thread::sleep(std::time::Duration::from_millis(700));
    }
}
