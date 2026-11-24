//! Last results per drive, so the Overview shows them the moment Pane opens.
//! `%LOCALAPPDATA%\Pane\scan-<letter>.json`, written atomically.

use std::path::{Path, PathBuf};

use crate::model::ScanResult;

pub fn dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Pane")
}

fn file_in(dir: &Path, volume: &str) -> PathBuf {
    let letter: String = volume.chars().filter(|c| c.is_ascii_alphabetic()).take(1).collect();
    dir.join(format!("scan-{}.json", letter.to_ascii_lowercase()))
}

pub fn load_from(dir: &Path, volume: &str) -> Option<ScanResult> {
    let text = std::fs::read_to_string(file_in(dir, volume)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_to(dir: &Path, result: &ScanResult) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = file_in(dir, &result.volume);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(result).map_err(std::io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

pub fn load(volume: &str) -> Option<ScanResult> {
    load_from(&dir(), volume)
}

pub fn save(result: &ScanResult) -> std::io::Result<()> {
    save_to(&dir(), result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregator;
    use crate::model::Mode;

    #[test]
    fn round_trips() {
        let dir = std::env::temp_dir().join(format!("pane-cache-{}", std::process::id()));
        let mut a = Aggregator::new(None);
        a.add_file(r"users\me\a.png", r"Users\me\a.png", 5, 0);
        let r = a.finish("C:", 100, 50, Mode::Standard, 1);
        save_to(&dir, &r).unwrap();
        assert_eq!(load_from(&dir, "C:"), Some(r));
        assert_eq!(load_from(&dir, "D:"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
