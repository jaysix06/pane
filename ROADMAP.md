# Pane roadmap

Where Pane is and what comes next. Latest release: **0.1.3** (thumbnails, preview pane, About, default file manager).

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

- [ ] Item checkboxes (the setting exists; the file views don't draw them yet)
- [ ] Language choice (Pane is English only for now)
- [ ] A log file (Pane's messages go nowhere in a release build)

## Next: Folder view redesign (A, B, C)

Three changes to the folder view, mocked up at https://claude.ai/artifact/5YA5RmgsAq3oj69kGALnRH (private to the owner). They keep Pane Amber, the LED dot-matrix and the keyboard model. Build them in this order; each can ship on its own. **Status: A is built (not yet committed or released); B and C are next.** Check each one in the running dev build (`target\debug\pane.exe`, not the installed copy).

### A. A command bar that follows the selection

The toolbar shows ten unlabeled icons at all times, half of them disabled with nothing selected. Instead:

- [x] Toolbar splits into two rows. Top capsule: back / forward / up / refresh, the path pill, search, More. Command row under it: commands on the left; Sort, the view switch and the preview toggle on the right. The command row is hidden on the Overview; on This PC it shows only the right side.
- [x] Nothing selected: an amber "New folder" button, and "Paste N items" ("Move N items here" after a cut) only while the clipboard holds files. Hidden in search results, which aren't a folder.
- [x] With a selection: a chip "3 files · 16 KB" (count in amber, "files" / "folders" / "items", the files' size; × clears the selection like Esc), then labelled Open, Copy, Cut, Rename (one item only), Delete. No disabled buttons are ever shown. Tooltips show the current key from `shortcuts.hint(id)`, so rebinding updates them.
- [x] Sort shows its current key as text ("Sort Name").
- [x] The search box is wider and its placeholder says "Search <folder> and subfolders".
- [x] Rust publishes `shared("clipboard")` = `{ count, cut }` (`app::refresh_clipboard`: checks the clipboard sequence number on selection changes, after copy / cut / paste, and when the window becomes active) and `sel.files`, `sel.folders`, `sel.size`.
- [ ] Later, if wanted: a "New" menu with more than New folder (text file and so on).
- Files: `ui/components/toolbar.luau`, `ui/shortcuts.luau` (`hint`), `ui/state.luau`, `crates/pane-app/src/app.rs`, `crates/pane-app/src/main.rs` (window-active effect), `crates/pane-app/src/ops.rs`, `crates/pane-shell/src/clipboard.rs` (`sequence`).
- Checked in the dev build: nothing selected, three files, one file (Rename shows), Copy → "Paste 3 items", This PC, Overview. Not checked: light theme, a narrow window.

### B. A list you can read at a glance

- [ ] Relative dates: "Today 9:26 AM", "Yesterday 4:10 PM", "Mon 10:37 PM" within a week, then "Nov 12" (and "Nov 12, 2025" in other years). Full timestamp in the tooltip. A setting keeps the old absolute format. An amber dot marks changes in the last hour.
- [ ] A Kind column replaces Type: folders show their item count ("9 items", counted lazily); files show a readable kind ("Markdown", "Lockfile") and a small mono extension tag after the name.
- [ ] Group headers when folders-first is on and sorted by name: "FOLDERS 8 ··· 5.0 GB" and "FILES 6 ··· 147 KB".
- [ ] Folder sizes with a 10-LED bar showing each folder's share of the current folder. **Needs new work:** the scan cache (`pane-scan` `ScanResult`) keeps only top items per category, not a size per folder. Plan: a background size worker like `thumbs.rs` (LIFO queue, generation counter, cancelled on navigation), leaving the cell blank until known; consider caching results by path + modified time.
- Files: `crates/pane-app/src/app.rs` (`row_text`, `ListOptions`), a new `crates/pane-app/src/sizes.rs`, `ui/components/views.luau`, `ui/components/meter.luau`, settings for the date format.

### C. A footer that knows about space

- [ ] The status bar becomes three cells with hairline dividers: selection ("14 items · 3 selected · 13 KB"), running operations, and the drive's free space ("C: [mini LED meter] 124 GB free", category colours as in the Overview; click opens the Overview).
- [ ] One running copy / move / delete shows inline in the footer: icon, "Copying to D:\Backup", a 24-LED progress bar, percent, time left. Two or more show "2 running"; clicking opens a popover above the footer with today's cards (pause / cancel). The cards no longer cover the file list. Windows' own conflict dialogs are unchanged.
- [ ] The sidebar drive row gets a readable meter: a 6 px dot row, free space right-aligned in mono, "352 of 476 GB used" below, and a highlight on the current drive.
- [ ] The operations cell shrinks first when the window narrows.
- Files: `ui/app.luau` (`status_bar`), `crates/pane-app/src/app.rs` (`status_text` splits into fields), `ui/components/ops_panel.luau`, `ui/components/sidebar.luau`, `ui/components/meter.luau`, `crates/pane-shell/src/places.rs`.

## Milestone 4 follow-ups

- [ ] Thumbnail cache on disk, so folders opened again after a restart don't wait (measure Windows' own cache first)
- [ ] Resizable preview pane (with resizable columns, below: the same drag handle)
- [ ] Syntax highlighting in code previews
- [ ] Try video previews and thumbnails across common formats

## Milestone 6: Polish and performance

- [ ] Idle CPU down from about 1% of a core to about 0%
- [ ] Show the first rows of huge folders while they're still loading (100k files take about 0.58 s now; target 100 ms)
- [ ] Bundle the UI scripts into the exe instead of shipping a `ui` folder
- [ ] Faster memory allocator (mimalloc) and quicker startup (ship Microsoft's shader compiler)
- [ ] Startup time and memory checks against Explorer

## Smaller leftovers

- [ ] Drag-select (rubber band) in the file views
- [ ] Resizable columns in details view
- [ ] Grid view: folder names without spaces are cut to one line
- [ ] Test drag and drop to and from other apps (desktop, Explorer)
- [ ] Test the temp-files "Clear…" button on a machine where deleting temp files is fine
- [ ] Code-sign the installer so Windows SmartScreen stops warning (needs a paid certificate)
- [x] Confirm the tester's two storage-bar reports are fixed in 0.1.2

## Later ideas

- [ ] Instant search across the whole drive (MFT plus the helper service)
- [ ] A background process so new windows open instantly
- [ ] Browsing inside zip files
- [ ] Phones and cameras (MTP)
