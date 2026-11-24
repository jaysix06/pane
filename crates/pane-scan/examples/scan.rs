//! Scans a drive and prints the breakdown:
//! `cargo run --release -p pane-scan --example scan -- C:\ [--mft]`
use std::sync::atomic::AtomicBool;

use pane_scan::{Category, Event};

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "C:\\".into());
    let report = |e: Event| match e {
        Event::Progress(p) => eprint!("\r{:>5.1}%  {} files   ", p.permille as f64 / 10.0, p.files),
        Event::Done(r) => {
            let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
            println!(
                "\n{}: {:.1} GB used of {:.1} GB · {} files, {} folders · {:.1} s ({:?})",
                r.volume, gb(r.used), gb(r.total), r.files, r.dirs, r.took_ms as f64 / 1000.0, r.mode
            );
            for c in Category::ALL {
                println!("  {:<10} {:>8.1} GB  {:>9} files", c.id(), gb(r.totals.bytes[c.index()]), r.totals.files[c.index()]);
            }
            println!("  unaccounted (in system): {:.1} GB", gb(r.unaccounted));
            for c in &r.cleanup {
                println!("  clear {:?}: {:.2} GB", c.kind, gb(c.bytes));
            }
        }
        Event::Error(e) => eprintln!("error: {e}"),
    };
    if std::env::args().any(|a| a == "--mft") {
        let volume = root.trim_end_matches('\\');
        if let Err(e) = pane_scan::mft::scan(volume, None, &report, &AtomicBool::new(false)) {
            eprintln!("mft: {e}");
        }
    } else {
        pane_scan::walk::scan(root.as_ref(), &report, &AtomicBool::new(false));
    }
}
