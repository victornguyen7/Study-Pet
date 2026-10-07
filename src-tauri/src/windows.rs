use tauri::{AppHandle, Manager};

// ---- Windows ----

#[cfg(target_os = "macos")]
pub(crate) fn move_to_active_space(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    if let Ok(ptr) = window.ns_window() {
        let ns_window = unsafe { &*(ptr as *const NSWindow) };
        let behavior = ns_window.collectionBehavior();
        ns_window.setCollectionBehavior(behavior | NSWindowCollectionBehavior::MoveToActiveSpace);
    }
}

#[tauri::command]
pub(crate) fn toggle_panel(app: AppHandle) -> tauri::Result<()> {
    let panel = app.get_webview_window("panel").expect("panel window in tauri.conf.json");
    if panel.is_visible()? {
        panel.hide()
    } else {
        panel.show()?;
        panel.set_focus()
    }
}
