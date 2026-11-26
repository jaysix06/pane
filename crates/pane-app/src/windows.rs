//! Windows the UI declares (`ui.window { id, open, ... }`): the Settings
//! window. Each opens while its `open` state is true and sets it false when
//! the person closes it. They share the app's state with the main window.
//! (zgx-run's `follow_windows`, with Pane's backdrop and nothing for devtools.)

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zgx_desktop::{Glass, GlassMaterial, WindowContext, WindowHandle, WindowOptions};
use zgx_script::{ScriptHost, Value};

/// Opens and closes `host`'s windows as their states say. Keep the returned
/// effect alive for as long as the app runs.
pub fn follow(host: &ScriptHost, cx: &WindowContext) -> zgx::reactive::Effect {
    let factory = cx.windows.clone();
    let runtime = cx.ui.runtime.clone();
    let open: Rc<RefCell<HashMap<String, WindowHandle>>> = Rc::default();
    let (list, host) = (host.windows(), host.clone());
    cx.ui.runtime.effect(move || {
        let windows = list.get();
        let gone: Vec<String> =
            open.borrow().keys().filter(|id| !windows.iter().any(|w| &w.id == *id)).cloned().collect();
        for id in gone {
            if let Some(handle) = open.borrow_mut().remove(&id) {
                runtime.untracked(|| handle.close());
            }
        }
        for window in windows {
            let wanted = window.open.get().truthy();
            let shown = open.borrow().get(&window.id).is_some_and(|h| !h.is_closed());
            if wanted && !shown {
                let (min_size, max_size) = host.window_size_limits(&window.id);
                let options = WindowOptions {
                    title: window.title.clone(),
                    min_size,
                    max_size,
                    width: window.width,
                    height: window.height,
                    frame_autosave: window.frame_autosave.clone(),
                    glass: Some(Glass::new(GlassMaterial::WindowBackground)),
                    ..Default::default()
                };
                let (view_host, state, id) = (host.clone(), window.open.clone(), window.id.clone());
                let handle = factory.open(options, move |wcx| {
                    wcx.on_closed(move || {
                        if state.with_untracked(Value::truthy) {
                            state.set(Value::Bool(false));
                        }
                    });
                    wcx.render(view_host.window_view(&id));
                });
                open.borrow_mut().insert(window.id.clone(), handle);
            } else if !wanted
                && shown
                && let Some(handle) = open.borrow_mut().remove(&window.id)
            {
                runtime.untracked(|| handle.close());
            }
        }
    })
}
