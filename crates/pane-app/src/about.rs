//! The About dialog's facts, and Pane as the default file manager.
//!
//! Published as `shared("about")`: `{ version, windows, default_fm,
//! default_error }`. The dialog asks for a refresh as it opens
//! (`about_refresh`), since another file manager may have taken folders back
//! since.

use std::cell::RefCell;
use std::rc::Rc;

use zgx_script::{ScriptHost, Shared, Value};

use pane_shell::default_app::{self, Root};

pub struct About {
    out: Shared,
    windows: String,
    error: RefCell<Option<String>>,
}

pub type AboutRef = Rc<About>;

/// This program, as the registry's commands name it.
fn exe() -> Option<String> {
    std::env::current_exe().ok().map(|p| p.display().to_string())
}

pub fn start(host: &ScriptHost) -> AboutRef {
    let about = Rc::new(About {
        out: host.shared("about", Value::Nil),
        windows: pane_shell::windows_version(),
        error: RefCell::new(None),
    });
    publish(&about);
    about
}

pub fn publish(about: &About) {
    let is_default = exe().is_some_and(|exe| default_app::is_default(Root::USER, &exe));
    let error = about.error.borrow();
    about.out.set(Value::record([
        ("version", Value::from(crate::updates::current().as_str())),
        ("windows", Value::from(about.windows.as_str())),
        ("default_fm", Value::from(is_default)),
        ("default_error", error.as_deref().map_or(Value::Nil, Value::from)),
    ]));
}

/// Makes Pane the default file manager (`on`), or gives folders back to
/// what had them before.
pub fn set_default(about: &About, on: bool) {
    let result = match exe() {
        None => Err("Couldn't find Pane's own program file.".to_string()),
        Some(exe) if on => default_app::set_default(Root::USER, &exe),
        Some(exe) => default_app::restore(Root::USER, Some(&exe)),
    };
    *about.error.borrow_mut() = result.err();
    publish(about);
}

/// `pane.exe --restore-file-manager`: the uninstaller gives folders back
/// before removing Pane (if this copy of Pane still has them).
pub fn restore_from_command_line() -> i32 {
    match exe() {
        Some(exe) => match default_app::restore(Root::USER, Some(&exe)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        None => 1,
    }
}
