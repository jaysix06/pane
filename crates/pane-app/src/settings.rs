//! Settings: a typed struct saved as `%APPDATA%\Pane\settings.json`.
//! Every page of the Settings window maps to fields here; the UI changes
//! one field at a time through `set_field`, which validates by
//! round-tripping through serde, so a wrong type is refused, not saved.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bump when a field changes meaning; `migrate` upgrades older files.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,

    // General
    /// "home", "this_pc", "downloads", or a folder path.
    pub start_folder: String,
    pub restore_session: bool,
    pub open_folders_in_new_tab: bool,
    pub single_click_open: bool,
    pub language: String,

    // Appearance
    /// "system", "light" or "dark".
    pub theme: String,
    /// "system" or "#rrggbb".
    pub accent: String,
    /// "mica", "mica_alt", "acrylic" or "solid".
    pub backdrop: String,
    /// "comfortable" or "compact".
    pub density: String,
    pub text_size: f64,
    /// "details", "list" or "grid".
    pub default_view: String,
    /// Grid tile icon size in points.
    pub icon_size: u32,
    /// "system", "on" or "off".
    pub animations: String,

    // Files & folders
    pub show_hidden: bool,
    pub show_system: bool,
    pub show_extensions: bool,
    pub folders_first: bool,
    pub item_checkboxes: bool,
    pub confirm_delete: bool,
    pub delete_permanently: bool,
    pub natural_sort: bool,

    // Sidebar
    pub sidebar_quick_access: bool,
    pub sidebar_known_folders: bool,
    pub sidebar_drives: bool,
    pub pinned: Vec<String>,

    // Thumbnails & preview
    pub thumbnails: bool,
    pub thumbnails_network: bool,
    pub preview_pane: bool,
    pub preview_max_kb: u32,

    // Search
    pub search_hidden: bool,
    pub search_case_sensitive: bool,
    pub search_whole_name: bool,
    pub search_max_depth: u32,

    // Keyboard shortcuts: command id to key, overriding the defaults.
    pub shortcuts: BTreeMap<String, String>,

    // Advanced
    pub thumbnail_threads: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: VERSION,
            start_folder: "home".into(),
            restore_session: true,
            open_folders_in_new_tab: false,
            single_click_open: false,
            language: "system".into(),
            theme: "system".into(),
            accent: "system".into(),
            backdrop: "mica".into(),
            density: "comfortable".into(),
            text_size: 14.0,
            default_view: "details".into(),
            icon_size: 64,
            animations: "system".into(),
            show_hidden: false,
            show_system: false,
            show_extensions: true,
            folders_first: true,
            item_checkboxes: false,
            confirm_delete: true,
            delete_permanently: false,
            natural_sort: true,
            sidebar_quick_access: true,
            sidebar_known_folders: true,
            sidebar_drives: true,
            pinned: Vec::new(),
            thumbnails: true,
            thumbnails_network: false,
            preview_pane: false,
            preview_max_kb: 1024,
            search_hidden: false,
            search_case_sensitive: false,
            search_whole_name: false,
            search_max_depth: 32,
            shortcuts: BTreeMap::new(),
            thumbnail_threads: 2,
        }
    }
}

/// What loading found, so the UI can say so.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadNote {
    Fresh,
    Loaded,
    /// The file could not be read as settings: it was kept as this backup.
    Recovered(PathBuf),
}

impl Settings {
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("Pane").join("settings.json")
    }

    /// Loads `path`. A missing file gives defaults; a broken one is moved
    /// aside as `settings.json.bak` and gives defaults.
    pub fn load(path: &Path) -> (Settings, LoadNote) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return (Settings::default(), LoadNote::Fresh),
        };
        match serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| serde_json::from_value::<Settings>(migrate(v)).ok())
        {
            Some(mut s) => {
                s.version = VERSION;
                s.sanitize();
                (s, LoadNote::Loaded)
            }
            None => {
                let backup = path.with_extension("json.bak");
                let _ = std::fs::rename(path, &backup);
                (Settings::default(), LoadNote::Recovered(backup))
            }
        }
    }

    /// Writes atomically: a temporary file beside it, renamed over it.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("settings serialize")
    }

    /// Sets one field by its name. Refuses unknown fields and values of the
    /// wrong type; clamps numbers and unknown choices into range.
    pub fn set_field(&mut self, key: &str, value: serde_json::Value) -> Result<(), String> {
        if key == "version" {
            return Err("version can't be set".into());
        }
        let mut json = self.to_json();
        let map = json.as_object_mut().expect("settings is an object");
        if !map.contains_key(key) {
            return Err(format!("unknown setting `{key}`"));
        }
        map.insert(key.to_string(), value);
        let mut next: Settings =
            serde_json::from_value(json).map_err(|e| format!("bad value for `{key}`: {e}"))?;
        next.sanitize();
        *self = next;
        Ok(())
    }

    /// Resets the named fields (a Settings page's "Reset this page").
    pub fn reset_fields(&mut self, keys: &[&str]) {
        let defaults = Settings::default().to_json();
        for key in keys {
            if let Some(v) = defaults.get(*key) {
                let _ = self.set_field(key, v.clone());
            }
        }
    }

    fn sanitize(&mut self) {
        fn one_of(value: &mut String, allowed: &[&str], default: &str) {
            if !allowed.contains(&value.as_str()) {
                *value = default.to_string();
            }
        }
        one_of(&mut self.theme, &["system", "light", "dark"], "system");
        one_of(&mut self.backdrop, &["mica", "mica_alt", "acrylic", "solid"], "mica");
        one_of(&mut self.density, &["comfortable", "compact"], "comfortable");
        one_of(&mut self.default_view, &["details", "list", "grid"], "details");
        one_of(&mut self.animations, &["system", "on", "off"], "system");
        if self.accent != "system" && !is_hex_color(&self.accent) {
            self.accent = "system".into();
        }
        self.text_size = self.text_size.clamp(11.0, 20.0);
        self.icon_size = self.icon_size.clamp(32, 128);
        self.preview_max_kb = self.preview_max_kb.clamp(16, 64 * 1024);
        self.search_max_depth = self.search_max_depth.clamp(1, 256);
        self.thumbnail_threads = self.thumbnail_threads.clamp(1, 8);
        self.pinned.dedup();
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Upgrades an older file's JSON to the current shape.
fn migrate(v: serde_json::Value) -> serde_json::Value {
    // Version 1 is the first; later versions add steps here, oldest first.
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pane-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn round_trips_every_field() {
        let path = temp("roundtrip");
        let mut s = Settings::default();
        s.show_hidden = true;
        s.theme = "dark".into();
        s.pinned = vec!["C:\\Work".into()];
        s.shortcuts.insert("new_tab".into(), "ctrl+t".into());
        s.save(&path).unwrap();
        let (loaded, note) = Settings::load(&path);
        assert_eq!(note, LoadNote::Loaded);
        assert_eq!(loaded, s);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let path = temp("partial");
        std::fs::write(&path, r#"{ "version": 1, "show_hidden": true }"#).unwrap();
        let (s, _) = Settings::load(&path);
        assert!(s.show_hidden);
        assert_eq!(s.theme, "system");
    }

    #[test]
    fn a_corrupt_file_is_backed_up_and_replaced_by_defaults() {
        let path = temp("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let (s, note) = Settings::load(&path);
        assert_eq!(s, Settings::default());
        let LoadNote::Recovered(backup) = note else { panic!("expected recovery") };
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "{ not json");
        assert!(!path.exists());
    }

    #[test]
    fn set_field_validates() {
        let mut s = Settings::default();
        assert!(s.set_field("show_hidden", serde_json::json!(true)).is_ok());
        assert!(s.show_hidden);
        assert!(s.set_field("show_hidden", serde_json::json!("yes")).is_err());
        assert!(s.set_field("nope", serde_json::json!(1)).is_err());
        assert!(s.set_field("version", serde_json::json!(9)).is_err());
        s.set_field("theme", serde_json::json!("purple")).unwrap();
        assert_eq!(s.theme, "system");
        s.set_field("text_size", serde_json::json!(99)).unwrap();
        assert_eq!(s.text_size, 20.0);
    }

    #[test]
    fn reset_fields_restores_defaults() {
        let mut s = Settings::default();
        s.show_hidden = true;
        s.theme = "dark".into();
        s.reset_fields(&["show_hidden"]);
        assert!(!s.show_hidden);
        assert_eq!(s.theme, "dark");
    }
}
