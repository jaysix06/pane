//! Which category a file belongs to, from its path (rules for well-known
//! places first, then its extension), and its "anchor": the folder the rule
//! is about (the game's folder, the node_modules folder, the app's folder),
//! which is what the Overview lists as the biggest places.

use crate::model::Category;

/// A file's category and how many leading path parts make its anchor
/// folder (`parts[..anchor]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Class {
    pub category: Category,
    pub anchor: usize,
}

fn class(category: Category, anchor: usize) -> Class {
    Class { category, anchor: anchor.max(1) }
}

const PHOTOS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "heic", "heif", "webp", "avif", "raw", "cr2", "cr3", "nef",
    "arw", "dng", "orf", "rw2", "psd", "jxl",
];
const VIDEOS: &[&str] = &["mp4", "mkv", "mov", "avi", "wmv", "webm", "m4v", "flv", "mts", "m2ts", "3gp", "mpg", "mpeg"];
const MUSIC: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "ogg", "wma", "opus", "aiff", "aif", "alac", "mid"];
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "rtf", "odt", "ods", "odp", "csv", "epub",
    "one", "pages", "numbers", "key", "tex",
];
/// Folder names that are developer tooling wherever they are.
const DEV_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".nuget",
    ".conda",
    "anaconda3",
    "miniconda3",
    ".venv",
    "venv",
    "__pycache__",
    ".android",
    ".pub-cache",
    ".dartserver",
    "go",
];
/// Folders whose contents are caches, under a user's AppData\Local.
const LOCAL_CACHES: &[&str] = &[
    "temp",
    "crashdumps",
    "d3dscache",
    "npm-cache",
    "pip",
    "yarn",
    "pnpm-cache",
    "nuget",
    "squirreltemp",
];
/// The cache folders browsers and Electron apps keep inside a profile.
const CACHE_LEAVES: &[&str] = &["cache", "code cache", "gpucache", "cache2", "shadercache", "grshadercache", "dawncache"];
/// Launchers whose library folders are games.
const GAME_LAUNCHERS: &[&str] = &["epic games", "riot games", "xboxgames", "gog galaxy", "ubisoft game launcher", "ea games"];

/// Classifies a file. `parts` are the lower-cased path components after
/// the drive ("users", "me", "desktop", "a.png"); the last is the file.
pub fn classify(parts: &[&str]) -> Class {
    let n = parts.len();
    if n == 0 {
        return class(Category::Other, 1);
    }
    let file = parts[n - 1];
    let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let first = parts[0];

    // The drive's own system areas.
    if first == "$recycle.bin" {
        return class(Category::Recycle, 1.min(n));
    }
    // NTFS metadata ($MFT, $LogFile, $Extend\…) and the page/hibernation files.
    if first.starts_with('$') {
        return class(Category::System, 1);
    }
    if n == 1 && matches!(file, "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "dumpstack.log" | "dumpstack.log.tmp") {
        return class(Category::System, 1);
    }
    if first == "system volume information" || first == "$windows.~bt" || first == "$windows.~ws" || first == "recovery" {
        return class(Category::System, 1);
    }
    if first == "windows" {
        if parts.get(1) == Some(&"temp") || parts.get(1) == Some(&"softwaredistribution") && parts.get(2) == Some(&"download") {
            return class(Category::Caches, 2.min(n - 1).max(1));
        }
        return class(Category::System, 1);
    }
    if first == "windows.old" {
        return class(Category::System, 1);
    }

    // Games: launcher libraries and Steam libraries, on any drive.
    if let Some(i) = parts.iter().position(|p| *p == "steamapps") {
        // steamapps\common\<Game>\…
        if parts.get(i + 1) == Some(&"common") && n > i + 3 {
            return class(Category::Games, i + 3);
        }
        return class(Category::Games, (i + 1).min(n - 1));
    }
    if let Some(i) = parts.iter().position(|p| GAME_LAUNCHERS.contains(p)) {
        return class(Category::Games, (i + 2).min(n - 1));
    }

    // Developer tooling anywhere.
    if let Some(i) = parts[..n - 1].iter().position(|p| DEV_DIRS.contains(p)) {
        // "go" only counts as a home-folder toolchain (C:\Users\me\go).
        if parts[i] != "go" || (first == "users" && i == 2) {
            return class(Category::Developer, i + 1);
        }
    }
    // Rust / CMake build output: …\target\debug|release\…
    if let Some(i) = parts[..n - 1].windows(2).position(|w| w[0] == "target" && matches!(w[1], "debug" | "release" | "doc")) {
        return class(Category::Developer, i + 1);
    }
    // WSL and Docker disks.
    if ext == "vhdx" && parts.iter().any(|p| p.contains("canonicalgroup") || p.contains("docker") || *p == "wsl") {
        return class(Category::Developer, (n - 1).max(1));
    }

    // Installed apps.
    if matches!(first, "program files" | "program files (x86)") {
        if parts.get(1) == Some(&"windowsapps") {
            return class(Category::Apps, 3.min(n - 1).max(1));
        }
        return class(Category::Apps, 2.min(n - 1).max(1));
    }
    if first == "programdata" {
        if matches!(parts.get(1), Some(&"package cache") | Some(&"microsoft")) && parts.get(2) != Some(&"windows defender") {
            return class(Category::Caches, 2.min(n - 1).max(1));
        }
        return class(Category::Apps, 2.min(n - 1).max(1));
    }

    // A user's profile.
    if first == "users" && n > 3 {
        let rest = &parts[2..];
        if rest[0] == "appdata" && rest.len() > 2 {
            let area = rest[1]; // local, locallow, roaming
            let vendor = rest[2];
            if area == "local" && LOCAL_CACHES.contains(&vendor) {
                return class(Category::Caches, 5.min(n - 1));
            }
            if area == "local" && vendor == "programs" {
                return class(Category::Apps, 6.min(n - 1));
            }
            // Browser and app caches inside a profile: …\Cache\…
            if let Some(j) = parts[..n - 1].iter().position(|p| CACHE_LEAVES.contains(p)) {
                return class(Category::Caches, j + 1);
            }
            // Windows' own caches (thumbnails, INetCache) and app packages' caches.
            if parts[..n - 1].iter().any(|p| matches!(*p, "inetcache" | "localcache" | "tempstate" | "dxcache" | "glcache")) {
                return class(Category::Caches, (n - 1).min(6));
            }
            if vendor == "microsoft" && rest.get(3) == Some(&"windows") && rest.get(4) == Some(&"explorer") {
                return class(Category::Caches, 7.min(n - 1));
            }
            // Media and documents an app keeps still count by kind below;
            // everything else is the app's data.
            if !(PHOTOS.contains(&ext) || VIDEOS.contains(&ext) || MUSIC.contains(&ext)) {
                return class(Category::Apps, 5.min(n - 1));
            }
        }
    }

    // By kind of file.
    let parent = (n - 1).max(1);
    if PHOTOS.contains(&ext) {
        return class(Category::Photos, parent);
    }
    if VIDEOS.contains(&ext) {
        return class(Category::Videos, parent);
    }
    if MUSIC.contains(&ext) {
        return class(Category::Music, parent);
    }
    if DOCUMENTS.contains(&ext) {
        return class(Category::Documents, parent);
    }
    class(Category::Other, parent.min(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(path: &str) -> (Category, String) {
        let lower = path.to_lowercase();
        let parts: Vec<&str> = lower.split('\\').filter(|p| !p.is_empty()).collect();
        let c = classify(&parts);
        (c.category, parts[..c.anchor].join("\\"))
    }

    #[test]
    fn system_places() {
        assert_eq!(of(r"pagefile.sys").0, Category::System);
        assert_eq!(of(r"Windows\System32\kernel32.dll"), (Category::System, "windows".into()));
        assert_eq!(of(r"Windows\Temp\x.tmp").0, Category::Caches);
        assert_eq!(of(r"$Recycle.Bin\S-1-5\$R1.zip").0, Category::Recycle);
        assert_eq!(of(r"System Volume Information\x").0, Category::System);
    }

    #[test]
    fn games_anchor_on_the_game() {
        assert_eq!(
            of(r"Program Files (x86)\Steam\steamapps\common\Dota 2 beta\game\x.vpk"),
            (Category::Games, r"program files (x86)\steam\steamapps\common\dota 2 beta".into())
        );
        assert_eq!(of(r"SteamLibrary\steamapps\common\PEAK\PEAK.exe").1, r"steamlibrary\steamapps\common\peak");
        assert_eq!(of(r"Riot Games\VALORANT\live\x.pak"), (Category::Games, r"riot games\valorant".into()));
        assert_eq!(of(r"Program Files\Epic Games\Fortnite\x.pak").0, Category::Games);
    }

    #[test]
    fn developer_folders() {
        assert_eq!(
            of(r"Users\me\code\app\node_modules\react\index.js"),
            (Category::Developer, r"users\me\code\app\node_modules".into())
        );
        assert_eq!(of(r"Users\me\.cargo\registry\x.crate"), (Category::Developer, r"users\me\.cargo".into()));
        assert_eq!(of(r"Users\me\proj\target\debug\app.exe"), (Category::Developer, r"users\me\proj\target".into()));
        assert_eq!(of(r"Users\me\go\pkg\mod\x.go").0, Category::Developer);
        assert_eq!(of(r"Users\me\photos\go\beach.jpg").0, Category::Photos, "only ~\\go is a toolchain");
    }

    #[test]
    fn apps_and_their_data() {
        assert_eq!(of(r"Program Files\Blender Foundation\Blender\blender.exe"), (Category::Apps, r"program files\blender foundation".into()));
        assert_eq!(of(r"Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe").1, r"users\me\appdata\local\programs\microsoft vs code");
        assert_eq!(of(r"Users\me\AppData\Roaming\discord\settings.json").0, Category::Apps);
    }

    #[test]
    fn caches() {
        assert_eq!(of(r"Users\me\AppData\Local\Temp\x\y.tmp"), (Category::Caches, r"users\me\appdata\local\temp".into()));
        assert_eq!(
            of(r"Users\me\AppData\Local\Google\Chrome\User Data\Default\Cache\Cache_Data\f_0001").1,
            r"users\me\appdata\local\google\chrome\user data\default\cache"
        );
        assert_eq!(of(r"Users\me\AppData\Local\npm-cache\_cacache\x").0, Category::Caches);
        assert_eq!(of(r"Users\me\AppData\Local\Microsoft\Windows\Explorer\thumbcache_96.db").0, Category::Caches);
    }

    #[test]
    fn media_and_documents_by_extension() {
        assert_eq!(of(r"Users\me\Pictures\2026\beach.HEIC"), (Category::Photos, r"users\me\pictures\2026".into()));
        assert_eq!(of(r"Users\me\Videos\clip.mkv").0, Category::Videos);
        assert_eq!(of(r"Users\me\Music\a.flac").0, Category::Music);
        assert_eq!(of(r"Users\me\Documents\report.pdf").0, Category::Documents);
        assert_eq!(of(r"Users\me\Downloads\setup.msi").0, Category::Other);
    }
}
