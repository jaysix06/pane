//! Pane: the window, the Luau UI host, and the actions the UI calls.
//! State and logic live in `app`; this file wires them together.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;
mod nav;
mod ops;
mod selection;
mod service;
mod settings;
mod storage;
mod updates;

use std::path::PathBuf;
use std::time::Duration;

use pane_fs::SortKey;
use zgx_desktop::{Application, Glass, GlassMaterial, Titlebar, WindowOptions};
use zgx_script::{ScriptHost, Value};

use crate::app::AppRef;
use crate::nav::Location;
use crate::selection::Move;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui_dir = ui_dir();
    let start = std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_dir());

    let mut application = Application::new();
    // Geist Mono (SIL OFL, assets/fonts) for readouts, paths and numbers.
    for bytes in [
        &include_bytes!("../../../assets/fonts/GeistMono-Regular.ttf")[..],
        &include_bytes!("../../../assets/fonts/GeistMono-Medium.ttf")[..],
        &include_bytes!("../../../assets/fonts/GeistMono-SemiBold.ttf")[..],
    ] {
        match zgx_desktop::FontData::new(bytes) {
            Ok(font) => application = application.font(font),
            Err(e) => eprintln!("bundled font failed to load: {e}"),
        }
    }
    application
        .window(WindowOptions {
            title: "Pane".into(),
            width: 1240.,
            height: 780.,
            min_size: Some((720., 460.)),
            titlebar: Titlebar::Inset,
            glass: Some(Glass::new(GlassMaterial::WindowBackground)),
            frame_autosave: Some("main".into()),
            ..Default::default()
        })
        .run(move |cx| {
            pane_shell::init_com();
            let host = ScriptHost::new(&cx.ui.runtime, ui_dir.join("app.luau")).app_id("pane");
            // `ui.window.titlebar_area` / `controls`: the tab strip stays clear
            // of Windows' caption buttons under the inset title bar.
            host.window_chrome(cx.window_info.titlebar_area.clone(), cx.window_info.controls.clone());
            host.window_state(&cx.ui);
            host.window_size(cx.viewport.clone());

            let app = app::App::start(&host, &cx.tasks, start.clone());
            let storage = storage::Storage::start(&host, &cx.tasks);
            let ops = ops::Ops::start(&host, &cx.tasks);
            let updates = updates::start(&host, &cx.tasks, &app);
            {
                let (u, a, t) = (updates.clone(), app.clone(), cx.tasks.clone());
                host.action("update_check", move |_| updates::check(&u, &a, &t));
                let u = updates.clone();
                host.action("update_open", move |_| updates::open(&u));
                let (u, a) = (updates.clone(), app.clone());
                host.action("update_dismiss", move |_| updates::dismiss(&u, &a));
            }
            register_actions(&host, &app, cx.window.clone());
            register_storage_actions(&host, &app, &storage, &ops);
            register_ops_actions(&host, &app, &ops);

            if let Err(e) = host.reload() {
                eprintln!("script error: {e}");
            }
            cx.tasks.spawn(host.watch(Duration::from_millis(250)));
            cx.render(host.view());
            cx.on_closed(move || {
                app::save_now(&app);
                host.save();
            });
        })
}

fn register_actions(host: &ScriptHost, app: &AppRef, window: zgx_desktop::WindowHandle) {
    // A helper: register `name` with a closure given the app and the argument.
    let on = |name: &str, f: fn(&AppRef, &Value)| {
        let app = app.clone();
        host.action(name, move |arg| f(&app, &arg));
    };

    // Navigation
    on("back", |a, _| app::back(a));
    on("forward", |a, _| app::forward(a));
    on("up", |a, _| app::up(a));
    on("refresh", |a, _| app::refresh(a));
    on("navigate", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::navigate(a, Location::from_ui(s));
        }
    });
    on("navigate_new_tab", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::new_tab_at(a, Location::from_ui(s), false);
        }
    });
    on("address", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::go_to_address(a, s);
        }
    });
    on("open", |a, arg| {
        if let Some(i) = index_arg(arg) {
            app::open_row(a, i, false);
        }
    });
    on("open_new_tab", |a, arg| {
        if let Some(i) = index_arg(arg) {
            app::open_row(a, i, true);
        }
    });
    on("open_selection", |a, _| app::open_selection(a));

    // Tabs
    on("new_tab", |a, _| app::new_tab(a));
    on("switch_tab", |a, arg| {
        if let Some(i) = index_arg(arg) {
            app::switch_tab(a, i);
        }
    });
    on("next_tab", |a, _| app::cycle_tab(a, 1));
    on("prev_tab", |a, _| app::cycle_tab(a, -1));
    {
        let (app, window) = (app.clone(), window.clone());
        host.action("close_tab", move |arg| {
            let i = index_arg(&arg).unwrap_or_else(|| app.borrow().active);
            if !app::close_tab(&app, i) {
                window.close();
            }
        });
    }

    // Selection: { index, ctrl, shift }
    on("click", |a, arg| {
        if let Some(i) = arg.field("index").and_then(index_arg) {
            app::click(a, i, flag(arg, "ctrl"), flag(arg, "shift"));
        }
    });
    on("select_all", |a, _| app::select_all(a));
    on("select_none", |a, _| app::select_none(a));
    on("move", |a, arg| {
        let by = match arg.field("key").and_then(str_arg) {
            Some("ArrowUp") => Move::Up,
            Some("ArrowDown") => Move::Down,
            Some("PageUp") => Move::PageUp,
            Some("PageDown") => Move::PageDown,
            Some("Home") => Move::Home,
            Some("End") => Move::End,
            _ => return,
        };
        let page = arg.field("page").and_then(num_arg).unwrap_or(10.0) as usize;
        app::move_focus(a, by, flag(arg, "shift"), page);
    });
    on("jump", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::jump(a, s);
        }
    });


    // Sort and view
    on("sort", |a, arg| {
        let key = match str_arg(arg) {
            Some("name") => SortKey::Name,
            Some("date") => SortKey::Modified,
            Some("type") => SortKey::Type,
            Some("size") => SortKey::Size,
            _ => return,
        };
        app::sort_by(a, key);
    });
    on("set_view", |a, arg| {
        if let Some(v) = str_arg(arg) {
            app::set_view(a, v);
        }
    });

    // Settings: { key, value } / { keys = {...} } (none: all)
    on("set_setting", |a, arg| {
        if let (Some(key), Some(value)) = (arg.field("key").and_then(str_arg), arg.field("value")) {
            app::set_setting(a, key, app::value_to_json(value));
        }
    });
    on("reset_settings", |a, arg| {
        let keys: Vec<String> = match arg.field("keys") {
            Some(Value::List(items)) => items.iter().filter_map(str_arg).map(String::from).collect(),
            _ => Vec::new(),
        };
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        app::reset_settings(a, &refs);
    });
    on("pin", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::pin(a, s);
        }
    });
    on("unpin", |a, arg| {
        if let Some(s) = str_arg(arg) {
            app::unpin(a, s);
        }
    });
    on("pin_current", |a, _| {
        let loc = a.borrow().tab().history.current.clone();
        if let Location::Folder(p) = loc {
            app::pin(a, &p.display().to_string());
        }
    });
}

/// The Overview's actions.
/// File operations, the clipboard, and the shell's menus.
fn register_ops_actions(host: &ScriptHost, app: &AppRef, ops: &ops::OpsRef) {
    let on = |name: &str, f: fn(&AppRef, &ops::OpsRef, &Value)| {
        let (app, ops) = (app.clone(), ops.clone());
        host.action(name, move |arg| f(&app, &ops, &arg));
    };
    on("copy", |a, o, _| ops::copy(o, a, false));
    on("cut", |a, o, _| ops::copy(o, a, true));
    on("paste", |a, o, _| ops::paste(o, a));
    // { permanent, confirmed }
    on("delete", |a, o, arg| ops::delete(o, a, flag(arg, "permanent"), flag(arg, "confirmed")));
    // { index, name }
    on("rename", |a, o, arg| {
        if let (Some(i), Some(name)) = (arg.field("index").and_then(index_arg), arg.field("name").and_then(str_arg)) {
            ops::rename(o, a, i, name);
        }
    });
    on("start_rename", |a, o, _| {
        let name = {
            let app = a.borrow();
            let tab = app.tab();
            tab.selection.focus().or_else(|| tab.selection.indices().next()).and_then(|i| tab.entries.get(i)).map(|e| e.name.clone())
        };
        if let Some(name) = name {
            ops::request_rename(o, &name);
        }
    });
    on("new_folder", |a, o, _| ops::new_folder(o, a));
    // { paths = { ... }, target, copy }: files dropped on a folder.
    on("drop", |a, o, arg| {
        let paths: Vec<PathBuf> = match arg.field("paths") {
            Some(Value::List(items)) => items.iter().filter_map(str_arg).map(PathBuf::from).collect(),
            _ => Vec::new(),
        };
        let target = match arg.field("target").and_then(str_arg) {
            Some(t) => PathBuf::from(t),
            None => match &a.borrow().tab().history.current {
                Location::Folder(p) => p.clone(),
                _ => return,
            },
        };
        ops::drop_files(o, a, paths, target, flag(arg, "copy"));
    });
    // The shell's menu for the row right-clicked (and the rest of the
    // selection when it is part of it).
    on("context_menu", |a, o, arg| {
        let Some(i) = index_arg(arg) else { return };
        app::ensure_selected(a, i);
        let (paths, folder) = {
            let app = a.borrow();
            let tab = app.tab();
            (tab.selected_paths(), tab.history.current.clone())
        };
        match pane_shell::show_context_menu(pane_shell::active_window(), &paths) {
            Ok(pane_shell::MenuOutcome::Rename) => {
                if let Some(name) = paths.first().and_then(|p| p.file_name()) {
                    ops::request_rename(o, &name.to_string_lossy());
                }
            }
            Ok(pane_shell::MenuOutcome::Invoked(_)) if matches!(folder, Location::Folder(_)) => app::refresh(a),
            Ok(_) => {}
            Err(e) => a.borrow().toast(&format!("The menu couldn't open: {e}")),
        }
    });
    // Right-click on empty space: the folder's own menu (New, Paste, …).
    on("background_menu", |a, _, _| {
        let folder = match &a.borrow().tab().history.current {
            Location::Folder(p) => p.clone(),
            _ => return,
        };
        app::select_none(a);
        match pane_shell::show_background_menu(pane_shell::active_window(), &folder) {
            Ok(pane_shell::MenuOutcome::Invoked(_)) => app::refresh(a),
            Ok(_) => {}
            Err(e) => a.borrow().toast(&format!("The menu couldn't open: {e}")),
        }
    });
}

fn register_storage_actions(host: &ScriptHost, app: &AppRef, storage: &storage::StorageRef, ops: &ops::OpsRef) {
    let on = |name: &str, f: fn(&AppRef, &storage::StorageRef, &Value)| {
        let (app, storage) = (app.clone(), storage.clone());
        host.action(name, move |arg| f(&app, &storage, &arg));
    };
    on("storage_scan", |_, s, _| storage::rescan(s));
    on("storage_select", |_, s, arg| {
        if let Some(v) = str_arg(arg) {
            storage::select(s, v);
        }
    });
    on("storage_drill", |_, s, arg| storage::drill(s, str_arg(arg)));
    // A folder or file from the Overview: opens in a new tab (a file's folder).
    on("storage_open", |a, _, arg| {
        if let Some(p) = str_arg(arg) {
            open_in_new_tab(a, p);
        }
    });
    on("cleanup_review", |a, s, arg| {
        if let Some(target) = str_arg(arg).and_then(|id| storage::cleanup_target(s, id)) {
            open_in_new_tab(a, &target);
        }
    });
    on("enable_instant_scan", |_, s, _| storage::set_instant(s, true));
    on("remove_instant_scan", |_, s, _| storage::set_instant(s, false));
    // Temporary files, cleared for good after the UI confirmed.
    {
        let (app, storage, ops) = (app.clone(), storage.clone(), ops.clone());
        host.action("clean_temp", move |arg| {
            if !flag(&arg, "confirmed") {
                return;
            }
            let folders = storage::cleanup_folders(&storage, "temp");
            let s2 = storage.clone();
            ops::purge_contents(&ops, &app, folders, Box::new(move |_| storage::rescan(&s2)));
        });
    }
    on("empty_recycle_bin", |_, s, _| {
        if pane_shell::empty_recycle_bin(pane_shell::active_window()) {
            storage::recycle_bin_emptied(s);
        }
    });
}

fn open_in_new_tab(app: &AppRef, path: &str) {
    let p = PathBuf::from(path);
    let folder = if p.is_dir() { Some(p) } else { p.parent().map(PathBuf::from) };
    if let Some(folder) = folder {
        app::new_tab_at(app, Location::Folder(folder), true);
    }
}

fn str_arg(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

fn num_arg(v: &Value) -> Option<f64> {
    match v {
        Value::Num(n) => Some(*n),
        _ => None,
    }
}

/// Lua indexes from 1; Rust from 0.
fn index_arg(v: &Value) -> Option<usize> {
    num_arg(v).filter(|n| *n >= 1.0).map(|n| n as usize - 1)
}

fn flag(arg: &Value, name: &str) -> bool {
    matches!(arg.field(name), Some(Value::Bool(true)))
}

/// The Luau UI: beside the executable in a release, the source tree in development.
fn ui_dir() -> PathBuf {
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("ui")))
        && dir.join("app.luau").exists()
    {
        return dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui")
}
