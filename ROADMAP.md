# Pane roadmap

Where Pane is and what comes next. Latest release: **0.3.0** (drag-select, resizable columns and preview pane, item checkboxes, instant whole-drive search, zip browsing and more; 0.2.x brought Settings, folder search, session restore, the folder view redesign, and Milestone 6's polish and performance).

**In 0.3.0:** drag-select, resizable columns and preview pane, item checkboxes, a New menu, folders sorted by measured size, dates that change at midnight, a log file, grid names that wrap, drives that show as they arrive, code highlighting in the preview, instant whole-drive search, and browsing inside zip files (each below). Built and tested in code (unit tests, a release build, the service's find end to end); **none of it has been checked on screen in the dev build yet**. Seen once while checking: `meter.luau:221` warns that a drive meter's animation keeps restarting; it shows on some launches and not others, with or without these changes.

## Done

- [x] **Milestone 1: Spike.** zgx window with the Windows 11 title bar, a 100,000-file list fed from Rust, Explorer's real right-click menu.
- [x] **Milestone 2: Core browsing.** Tabs, history, editable path bar, sidebar (Quick access, folders, drives), details / list / grid views, sorting, Explorer-style selection and keyboard shortcuts, folder watching, real file icons, typed settings file.
- [x] **Milestone 2.5: Pane's own design and the Storage Overview.** Amber dot-matrix design language, light theme, customizer (accent, glow, corners, density, motion, backdrop), Storage Overview home with categories and drill-down, cleanup cards, instant MFT scans through the helper service.
- [x] **Milestone 3: File operations.** Copy / cut / paste (Explorer-compatible clipboard), delete to Recycle Bin or permanently, rename in place, new folder, drag and drop with spring-loaded folders, background right-click menu, progress cards, temp-file cleanup. Plus a full UI audit pass.
- [x] **Extras.** Public GitHub repo (MIT), app icon, Pane Setup installer and uninstaller, update checks with a sidebar card, in-app download / verify / install / restart.
- [x] **Milestone 4: Thumbnails and preview pane.** Picture and video thumbnails in grid view, made only for what's on screen and kept in a memory cache. A preview pane (Alt+P): pictures, text and code up to a size limit, icons, and file details. Details view drops columns when narrow.
- [x] **Default file manager.** About → "Open folders in Pane": folders, drives and Win+E open in Pane; turning it off (or uninstalling) gives them back to the app that had them.

## Next: Milestone 5, Settings, About, search and session

- [x] Settings window in the Windows 11 Settings style (Ctrl+,), with a search box and "Reset this page":
  - [x] General: start folder, restore last session, open folders in a new tab, single or double click, open folders in Pane
  - [x] Appearance: reuses the customizer, plus layout for new tabs and grid icon size
  - [x] Files & folders: hidden and system files, extensions, folders first, delete confirmation, Recycle Bin or permanent, natural sort
  - [x] Sidebar: show or hide sections, reorder and unpin pins
  - [x] Thumbnails & preview: on / off, network drives, preview pane, text preview size limit
  - [x] Search: hidden files, case sensitivity, whole name, maximum depth
  - [x] Keyboard shortcuts: rebinding with conflict warnings
  - [x] Advanced: thumbnail threads, automatic update checks, clear caches, settings folder, export / import, reset everything
- [x] About page: version, "Copy version info", open-source licenses, links, "Check for updates" button
- [x] Search of the current folder (Ctrl+F), streaming results into the tab, cancellable, `*` and `?` wildcards
- [x] Reopen your tabs on launch (window position was already kept)

Left for later from Milestone 5:

- [x] Item checkboxes: Files & folders → "Item checkboxes" draws a box on each row (details, list) and tile (grid); a click adds or removes the item as Ctrl+click does. Details view's header has a select-all box.
- [ ] Language choice (Pane is English only for now)
- [x] A log file: a release build sends its messages (and the UI scripts' errors) to `%APPDATA%\Pane\pane.log`, set aside as `pane.old.log` past 1 MB (`pane_shell::log_to_file`). Settings → Advanced → "Log file" opens it. A debug build keeps the console.

## Next: Folder view redesign (A, B, C)

Three changes to the folder view, mocked up at https://claude.ai/artifact/5YA5RmgsAq3oj69kGALnRH (private to the owner). They keep Pane Amber, the LED dot-matrix and the keyboard model. Build them in this order; each can ship on its own. **Status: A, B and C are built and released in 0.2.0.** Check each one in the running dev build (`target\debug\pane.exe`, not the installed copy).

### A. A command bar that follows the selection

The toolbar shows ten unlabeled icons at all times, half of them disabled with nothing selected. Instead:

- [x] Toolbar splits into two rows. Top capsule: back / forward / up / refresh, the path pill, search, More. Command row under it: commands on the left; Sort, the view switch and the preview toggle on the right. The command row is hidden on the Overview; on This PC it shows only the right side.
- [x] Nothing selected: an amber "New folder" button, and "Paste N items" ("Move N items here" after a cut) only while the clipboard holds files. Hidden in search results, which aren't a folder.
- [x] With a selection: a chip "3 files · 16 KB" (count in amber, "files" / "folders" / "items", the files' size; × clears the selection like Esc), then labelled Open, Copy, Cut, Rename (one item only), Delete. No disabled buttons are ever shown. Tooltips show the current key from `shortcuts.hint(id)`, so rebinding updates them.
- [x] Sort shows its current key as text ("Sort Name").
- [x] The search box is wider and its placeholder says "Search <folder> and subfolders".
- [x] Rust publishes `shared("clipboard")` = `{ count, cut }` (`app::refresh_clipboard`: checks the clipboard sequence number on selection changes, after copy / cut / paste, and when the window becomes active) and `sel.files`, `sel.folders`, `sel.size`.
- [x] A "New" menu beside New folder: Folder, Text document, Markdown document (`ops::new_file`, numbered as Explorer does: "New Text Document (2).txt", `unique_file_name`, tested), selected and renaming once made. Not Explorer's full ShellNew list (Word and so on).
- Files: `ui/components/toolbar.luau`, `ui/shortcuts.luau` (`hint`), `ui/state.luau`, `crates/pane-app/src/app.rs`, `crates/pane-app/src/main.rs` (window-active effect), `crates/pane-app/src/ops.rs`, `crates/pane-shell/src/clipboard.rs` (`sequence`).
- Checked in the dev build: nothing selected, three files, one file (Rename shows), Copy → "Paste 3 items", This PC, Overview. Not checked: light theme, a narrow window.

### B. A list you can read at a glance

- [x] Friendly dates: "Today 9:26 AM", "Yesterday 4:10 PM", "Mon 10:37 PM" within a week, then "Nov 12" ("Nov 12, 2025" in other years). The full timestamp is the tooltip. An amber dot and brighter text mark changes in the last hour. Setting: Files & folders → "Friendly dates" (`relative_dates`). Logic in `pane-fs/src/date.rs`, tested.
- [x] The Type column is now Kind: subfolders show their item count ("9 items", "Empty"); files show a readable kind ("Markdown", "Lockfile", "Git file"; `kind_name` in `app.rs`, tested). When names hide extensions, the extension shows as a small mono tag after the name.
- [x] Group headers in details view when folders come first, in folders (not search, This PC or the Overview): "FOLDERS 8 ──── 21 GB" ("Measuring…" until done) and "FILES 6 ──── 148 KB". Done with the list's per-row `heights` (the first folder and first file rows are taller), so row indexes, selection and keys are unchanged. Skipped past 20,000 rows.
- [x] Folder sizes: `crates/pane-app/src/sizes.rs` measures the folder on screen on one background thread (lowered CPU and disk priority): item counts first, then sizes (`pane_fs::folder_size`, which doesn't follow junctions or symlinks), published as `shared("sizes")`. Ten LEDs show each folder's share of the folder, rounded to tenths, once every size is known. A new folder cancels the walk; sizes are kept 2 minutes. Not on network drives. Setting: "Show folder sizes" (`folder_sizes`). Home (56 folders, 135 GB) takes about 41 s here; the project folder is instant.
- [x] Sorting by size uses measured folder sizes: once sizes.rs knows every subfolder's size it says so (`Sizes::take_done`), and a Size sort puts the folders in order then, keeping the selection (`sort_by_folder_sizes`, `pane_fs::sort_entries_by_size`, tested). A refresh of a measured folder sorts at once.
- [x] Friendly dates change at midnight: every tab's rows are built again then (`midnight_loop`, `rebuild_rows`). The amber "last hour" dot still only updates when a folder is listed again.
- [ ] Later: a change deep inside a subfolder shows up in its size only after the 2-minute cache ends.
- Files: `crates/pane-fs/src/date.rs`, `crates/pane-fs/src/size.rs`, `crates/pane-app/src/sizes.rs`, `crates/pane-app/src/app.rs` (`row_text`, `kind_name`, `publish_groups`, `request_sizes`), `crates/pane-app/src/settings.rs`, `ui/components/views.luau`, `ui/state.luau` (`sizes`, `groups`, `folder_info`), `ui/settings.luau`.
- Checked in the dev build: the project folder, Home (sizes, groups, End / Page Up across the FILES header), list view, This PC, search results. Not checked: light theme, a narrow window, a network drive.

### C. A footer that knows about space

- [x] The status bar is three cells with hairline dividers: selection ("14 items · 3 selected · 13 KB", from Rust's `shared("status")` = `{ items, selected, size }`), file operations, and the drive's free space ("C: [mini LED meter] 124 GB free", category colours once the Overview has scanned the drive; a click opens the Overview on that drive). On the Overview the footer appears only while an operation runs.
- [x] One running copy / move / delete shows inline: icon, "Copying to D:\Backup", a 24-LED progress bar, percent, time left ("15 s left", `ops::time_left`, tested; paused time doesn't count). Two or more show "2 running". A click opens a popover above the footer with today's operations, newest first, each with pause / resume and cancel, plus "Clear finished". A finished one stays in the footer 2.5 s (8 s if it failed). The floating cards are gone. Windows' conflict dialogs are unchanged.
- [x] Pause and cancel: `pane_shell::fileops::Control`, checked before each item and on each progress report. A paused operation waits there; a cancelled one stops before its next item (Windows finishes or rolls back the item in hand).
- [x] The sidebar drive row: name with free space right-aligned in mono, a one-row LED meter (6 px), "352 of 476 GB used" below; the drive the folder on screen is on gets a bolder name and amber free space. Drives are always published now (the footer needs them); `places.show_drives` hides them in the sidebar. Free space is read again when an operation ends.
- [x] The operations cell shrinks first when the window narrows.
- Files: `ui/app.luau` (`status_bar`, `space_cell`), `ui/components/ops_panel.luau` (`cell`, `active`), `ui/components/sidebar.luau` (`drive_row`), `ui/components/meter.luau` (`drive_data`), `ui/components/icons.luau` (pause, play), `ui/state.luau`, `crates/pane-app/src/app.rs` (`status_value`, `used_of`, `refresh_places`), `crates/pane-app/src/ops.rs`, `crates/pane-app/src/main.rs` (`op_pause`, `op_cancel`, `ops_clear`), `crates/pane-shell/src/fileops.rs` (`Control`, `run_with`), `crates/pane-fs/src/date.rs` (`clock`).
- Checked in the dev build: the footer in a folder (selection cell, drive cell), the sidebar drives, copying 3 × 1.5 GB files (bar, percent, "Done", free space updated right after), copying 6,000 small files (time left, popover, pause holds the copy, Stop cancels it, Cancel in Windows' conflict dialog reads "Cancelled"), a narrow window (the operations label truncates first). Tests: `time_left`, `short_path`, `used_of`, and cancel / pause-then-cancel of a folder copy in `fileops`.
- Not checked: two operations at once ("2 running"), a failed operation, the Overview's footer, the light theme. Whether Stop interrupts one very large file partway through or waits for the next item.
- Found while checking: a button labelled "Cancel" inside a zgx popover never runs its on_click (zgx seems to take it as the popover's dismiss button), so the card's button is labelled "Stop". Windows' conflict dialog for a big folder copy comes up behind Pane until Pane's window is clicked (seen once, likely older than this change).
- Later: "Skip" on every conflict in Windows' dialog may also read "Cancelled" (`GetAnyOperationsAborted`).

## Milestone 4 follow-ups

- [x] ~~Thumbnail cache on disk~~: not needed. Measured Windows' own cache first (`cargo run -p pane-shell --release --example thumbtime -- <folder>`, 2026-11-27): a new process gets a 256 px thumbnail of a picture made before in 4–10 ms, 1–4.5 ms once warm. A cache of Pane's own (256 KB a bitmap) would save a few ms a tile.
- [x] Resizable preview pane: drag its left edge (220–720 points, the `preview_width` setting); a double click puts it back to 320.
- [x] Syntax highlighting in code previews (`ui/components/highlight.luau`, zgx's textarea `highlight`): keywords, strings, comments (block comments and template strings run on across lines), numbers and capitalised type names, for Rust, C/C++/Java/Kotlin/Swift, C#, JavaScript/TypeScript, Go, Python, shell/PowerShell, Lua/Luau, SQL, JSON and config files (TOML, INI, YAML). Colours are `$code.*` theme tokens, one set per theme. Not checked on screen.
- [ ] Try video previews and thumbnails across common formats

## Milestone 6: Polish and performance

- [x] Idle CPU down to about 0%. The 15-second drive refresh re-published free space that moves by a few bytes, so the drive meters re-animated about 175 frames each time. Drive fullness is now rounded to thousandths (`used_share`, tested) and an unchanged reading publishes nothing: at rest Pane presents no frames (`ZGX_PRESENT_LOG`) and used one 15 ms scheduler tick in 60 s (0.03%).
- [x] The first rows of huge folders show while the rest loads. A navigation into a folder of more than 1,000 entries publishes the first 1,000, sorted, as soon as they're read (`pane_fs::list_dir_early`, tested; `app::show_early`), then the whole folder replaces them, keeping what was selected. 100k files: the first rows ~20 ms after listing starts, and they're in the window's first frame; before, nothing showed until all 100k were listed and published (~330 ms). Not for refreshes, which keep the rows on screen. Group headers and folder sizes wait for the full listing.
- [x] UI scripts bundled into the exe: a release build compiles `ui/` to Luau bytecode in `build.rs` (`zgx_script::build::bundle`, a script that doesn't parse fails the build) and runs it with `ScriptHost::bundled`. Debug builds still read `ui/` and reload on save. The installer no longer ships a `ui` folder and removes an older install's.
- [x] mimalloc as the allocator (zgx-desktop's `mimalloc` feature) and Microsoft's shader compiler shipped beside `pane.exe` (`dxcompiler.dll` 1.8.2502 from the Windows SDK, copied by `scripts/build-installer.ps1`; zgx uses it over FXC when present). mimalloc: about 10% less CPU opening 100k files, for 25–45 MB more private memory. DXC matters for a cold start (no shader cache); warm launches read shaders from the cache either way.
- [x] Startup time and memory checks against Explorer. Measured 2026-11-27, release build, median of 5 launches after a warm-up; "first frame" from `ZGX_PROFILE`; "ready" = the process's CPU quiet for 500 ms; memory at ready:

  | | Window created | First frame | Ready | CPU to ready | Private memory |
  |---|---|---|---|---|---|
  | Pane 0.1.3 code, 30 files | 63 ms | 161 ms | 1.37 s | 0.53 s | 88 MB |
  | Pane 0.1.3 code, 100k files | 46 ms | 168 ms (empty list) | 1.53 s | 0.98 s | 234 MB |
  | Pane now, 30 files | 62 ms | 169 ms | 1.39 s | 0.59 s | 135 MB |
  | Pane now, 100k files | 62 ms | 197 ms (first 1,000 rows) | 1.44 s | 0.81 s | 252–292 MB |
  | Explorer, 30 files | 847 ms (window with title) | | | | 128 MB (own process) |
  | Explorer, 100k files | 629 ms (window with title) | | | | 153 MB (own process, still filling) |

  Pane's window is up more than ten times sooner than Explorer's. Pane uses more memory than Explorer on a 100k folder (its rows are all built at once; Explorer fills in as it goes) and mimalloc adds to it. "Ready" is about 1.4 s for both builds because of the start-up animations (about 175 frames in the first 1.5 s), not loading work.
- Earlier measurement, against File Pilot (same method, before this milestone):

  | | Window created | Ready | CPU to ready | Private memory | Idle CPU |
  |---|---|---|---|---|---|
  | Pane, 30 files | 47 ms | 1.29 s | 0.66 s | 91 MB | 0.16% |
  | Pane, 100k files | 59 ms | 1.42 s | 1.20 s | 232 MB | 0.31% |
  | File Pilot, 30 files | 471 ms | 0.56 s | 1.28 s | 462 MB | 0.31% |
  | File Pilot, 100k files | 486 ms | 0.72 s | 1.55 s | 454 MB | 0.47% |

Left for later from Milestone 6:

- [ ] Memory with 100k files: rows are built as script values all at once; build them for what's on screen (or more compactly) instead.
- [ ] Start-up animations keep the window drawing for about 1.5 s after launch.
- [x] Drives that arrive or go (a USB stick) show at once: a hidden window on its own thread hears `WM_DEVICECHANGE` (`pane_shell::watch_devices`), and the sidebar reads the drives 0.8 s later. Free space other apps change is read every 60 s (was 15 s); Pane's own operations read it as they end. Not checked with a real stick.
- [ ] Measure a cold start (empty shader cache) with DXC against FXC on this machine.

## Smaller leftovers

- [x] Drag-select (rubber band) in the file views: a press on empty space (in details view, anywhere on a row but its icon and name, as in Explorer) and a drag draws an amber rectangle and selects what it touches; Ctrl or Shift keeps the selection there was. The list scrolls while the pointer is past its top or bottom. The rows covered are worked out in `views.luau` (`covered`) and sent as ranges (`marquee_begin`, `marquee`, `marquee_end`; `Selection::marquee`, tested). Not checked on screen: a release outside the window may leave the rectangle until the next press.
- [x] Resizable columns in details view: drag the hairline left of Date modified, Kind or Size (60–480 points, saved as `col_date`, `col_kind`, `col_size`); a double click puts one back. Name takes the rest.
- [x] Grid view: long names without spaces wrap to two lines (zero-width spaces after `_ - .`, or every 10 letters, in the shown name only).
- [ ] Test drag and drop to and from other apps (desktop, Explorer)
- [ ] Test the temp-files "Clear…" button on a machine where deleting temp files is fine
- [ ] Code-sign the installer so Windows SmartScreen stops warning (needs a paid certificate)
- [x] Confirm the tester's two storage-bar reports are fixed in 0.1.2

## Later ideas

- [x] Instant search across the whole drive: a search from a drive's root (C:\) asks the helper service (`{"find":{...}}`, `pane_scan::find`), which reads the drive's MFT once and keeps the table 90 s for the next keystrokes. It leaves out NTFS's own files, other users' profiles and (unless Search → hidden files) hidden items and what's in hidden folders, and checks each match as the caller (impersonated), so only what they could open comes back; at most 5,000. Folder searches and the folder walk share one `Matcher`. Settings → Search → "Instant search of whole drives". Without the service, or with an older one that only scans, the drive is walked as before. Measured with `pane-scan-service debug-serve` and `cargo run -p pane-scan --example find -- C: readme`: 4.3 s the first time (reading C:'s table), 122 ms after. **The installed service must be updated (a new installer) before Pane uses it.** Changes made after the table is read don't show until it's read again.
- [x] The Recycle Bin in Pane (after 0.3.0): a sidebar row shows how full it is ("12 items · 340 MB", `SHQueryRecycleBinW`) and opens it in a tab. Pane reads each drive's bin itself (`X:\$Recycle.Bin\<SID>`: an item is a `$R` file or folder and a `$I` record of its original path, size and deletion time, versions 1 and 2; `pane_shell::recycle`, tested, and matched Windows' own count here). Rows show the name it had, Original location and Date deleted. Restore puts items back ("name (2)" if the name is taken now, its folder made again if it's gone); Delete permanently (also the Delete key, after asking) runs as a normal operation with a progress card; Empty Recycle Bin uses Windows' own (it asks). Right-click: Restore, Delete permanently. Files dropped on the sidebar row or the bin's view go to the bin. Copy, Cut, Rename, New, search and dragging out are off there. The row's menu can still open it in File Explorer; Settings → Sidebar hides it. Not checked on screen.
- [ ] A background process so new windows open instantly
- [x] Browsing inside zip files, read-only (`pane_fs::zip`, tested): a .zip opens as a folder, its folders open, a file in it is taken out to `%TEMP%\Pane\zip\` and opened, and Copy takes the selection out there and puts those files on the clipboard. Delete, Rename, Cut, Paste, New and the right-click menus say it's read-only or do nothing. Stored and deflated entries only. Not checked on screen; dragging items out of a zip doesn't work yet.
- [ ] Phones and cameras (MTP)
