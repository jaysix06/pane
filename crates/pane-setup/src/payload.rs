//! The files Setup carries (packed by build.rs).

/// The compressed payload built into this program.
pub static PACKED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));

/// One file to install: its relative path (`/`-separated) and contents.
pub struct File {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Unpacks a payload; refuses anything malformed or with a path that would
/// leave the install folder.
pub fn unpack(packed: &[u8]) -> Result<Vec<File>, String> {
    let raw = miniz_oxide::inflate::decompress_to_vec(packed).map_err(|e| format!("damaged installer ({e:?})"))?;
    let mut at = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let end = at.checked_add(n).filter(|&e| e <= raw.len()).ok_or("damaged installer (truncated)")?;
        let s = &raw[at..end];
        at = end;
        Ok(s)
    };
    if take(5)? != b"PANE1" {
        return Err("damaged installer (not a Pane payload)".into());
    }
    let count = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
        let name = String::from_utf8(take(name_len)?.to_vec()).map_err(|_| "damaged installer (name)")?;
        if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':') || name.split('/').any(|p| p == "..") {
            return Err(format!("damaged installer (bad path {name})"));
        }
        let size = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        files.push(File { name, bytes: take(size)?.to_vec() });
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut raw = b"PANE1".to_vec();
        raw.extend_from_slice(&(files.len() as u32).to_le_bytes());
        for (n, b) in files {
            raw.extend_from_slice(&(n.len() as u16).to_le_bytes());
            raw.extend_from_slice(n.as_bytes());
            raw.extend_from_slice(&(b.len() as u64).to_le_bytes());
            raw.extend_from_slice(b);
        }
        miniz_oxide::deflate::compress_to_vec(&raw, 6)
    }

    #[test]
    fn round_trips() {
        let files = unpack(&pack(&[("pane.exe", b"MZ"), ("ui/app.luau", b"return 1")])).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].name, "ui/app.luau");
        assert_eq!(files[1].bytes, b"return 1");
    }

    #[test]
    fn refuses_paths_that_escape() {
        assert!(unpack(&pack(&[("../evil.exe", b"x")])).is_err());
        assert!(unpack(&pack(&[("C:/evil.exe", b"x")])).is_err());
        assert!(unpack(&pack(&[("a\\b", b"x")])).is_err());
        assert!(unpack(b"garbage").is_err());
    }

    #[test]
    fn the_built_in_payload_has_pane() {
        let files = unpack(PACKED).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        for want in ["pane.exe", "pane-scan-service.exe", "ui/app.luau", "ui/theme.luau", "LICENSE.txt"] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
    }
}
