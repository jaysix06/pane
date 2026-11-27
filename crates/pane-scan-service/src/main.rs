//! Pane's helper service: reads NTFS master file tables for instant
//! storage scans, which needs admin rights Pane itself doesn't have.
//!
//!   pane-scan-service install     (elevated, from Pane's "Enable instant scan")
//!   pane-scan-service uninstall   (elevated, from Settings)
//!   pane-scan-service run --allow <SID>   (started by Windows, not by hand)
//!
//! Security: it runs as LocalSystem, so it does as little as possible. It
//! is installed into Program Files (not a user-writable folder); it serves
//! one local pipe whose ACL admits only the installing user, SYSTEM and
//! Administrators; it accepts only `{"scan":"X:"}` and a name search of a
//! drive (`{"find":{...}}`, checked by `FindRequest::valid`); it names no
//! file in another user's profile (those count as one "other users" total),
//! and a search returns only what the caller can open, checked as the
//! caller; and it stops itself after a minute without requests.

#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let code = match args.first().map(String::as_str) {
            Some("install") => win::report(win::install()),
            Some("uninstall") => win::report(win::uninstall()),
            // Diagnostics: the same pipe loop in a console, printing errors.
            Some("debug-serve") => {
                let allow = args.iter().position(|a| a == "--allow").and_then(|i| args.get(i + 1)).cloned();
                win::debug_serve(allow.unwrap_or_default())
            }
            Some("run") => {
                let allow = args.iter().position(|a| a == "--allow").and_then(|i| args.get(i + 1)).cloned();
                match allow {
                    Some(sid) => win::run_service(sid),
                    None => {
                        eprintln!("run needs --allow <SID>");
                        2
                    }
                }
            }
            _ => {
                eprintln!("Pane's helper service. Install it from Pane (Overview → Enable instant scan).");
                2
            }
        };
        std::process::exit(code);
    }
    #[cfg(not(windows))]
    {
        eprintln!("Windows only");
        std::process::exit(2);
    }
}
