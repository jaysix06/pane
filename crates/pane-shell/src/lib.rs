//! The Windows shell: the real right-click menu, opening files, and other
//! things only Explorer's own APIs can do.

#[cfg(windows)]
mod context_menu;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use context_menu::{MenuOutcome, show_context_menu};
#[cfg(windows)]
pub use win::{active_window, init_com, open_path};
