//! Pane: the window, the Luau UI host, and the actions the UI calls.
//! State and logic live in `app`; this file wires them together.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icons;
mod nav;
mod selection;
mod settings;

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

    Application::new()
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
            register_actions(&host, &app, cx.window.clone());

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

    // The shell's menu for the row right-clicked (and the rest of the
    // selection when it is part of it).
    on("context_menu", |a, arg| {
        let Some(i) = index_arg(arg) else { return };
        app::ensure_selected(a, i);
        let (paths, folder) = {
            let app = a.borrow();
            let tab = app.tab();
            (tab.selected_paths(), tab.history.current.clone())
        };
        match pane_shell::show_context_menu(pane_shell::active_window(), &paths) {
            Ok(pane_shell::MenuOutcome::Invoked(_)) if matches!(folder, Location::Folder(_)) => app::refresh(a),
            Ok(_) => {}
            Err(e) => a.borrow().toast(&format!("The menu couldn't open: {e}")),
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
