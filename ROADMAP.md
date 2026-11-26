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
