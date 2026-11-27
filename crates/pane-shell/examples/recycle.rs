//! Lists this user's Recycle Bin as Pane reads it, beside what Windows
//! says it holds: `cargo run -p pane-shell --example recycle`
fn main() {
    let items = pane_shell::recycle::list();
    for f in pane_shell::recycle::bin_folders() {
        println!("bin: {}", f.display());
    }
    for i in items.iter().take(10) {
        println!("  {}{}  from {}  ({} bytes)", i.name(), if i.is_dir { "\\" } else { "" }, i.from().display(), i.size);
    }
    let bytes: u64 = items.iter().map(|i| i.size).sum();
    println!("Pane: {} items, {bytes} bytes", items.len());
    println!("Windows: {:?} (items, bytes)", pane_shell::recycle_bin_info());
}
