//! Independent, singleton settings window and the native macOS menu entry.

use tauri::{
    AppHandle, Manager, Runtime, WebviewUrl,
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    webview::WebviewWindowBuilder,
};

pub const SETTINGS_WINDOW_LABEL: &str = "settings";
const SETTINGS_MENU_ID: &str = "settings-open";

/// Show the existing settings window or create it from the same frontend
/// bundle used by the main window.
pub fn open_settings_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("index.html?window=settings".into()),
    )
    .title("设置")
    .inner_size(760.0, 580.0)
    .min_inner_size(700.0, 500.0)
    .resizable(true)
    .build()?;
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// Build the native app menu.  The item id is shared by the command and the
/// global menu handler so both entry points always use one window helper.
pub fn install_native_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let settings_item = MenuItemBuilder::with_id(SETTINGS_MENU_ID, "设置…")
        .accelerator("Cmd+,")
        .build(app)?;
    let app_submenu = SubmenuBuilder::new(app, "Double Love Studio")
        .item(&settings_item)
        .separator()
        .quit()
        .build()?;
    let edit_submenu = SubmenuBuilder::new(app, "编辑")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let menu = MenuBuilder::new(app)
        .item(&app_submenu)
        .item(&edit_submenu)
        .build()?;
    app.set_menu(menu)?;
    Ok(())
}

pub fn is_settings_menu_item(id: &str) -> bool {
    id == SETTINGS_MENU_ID
}

#[tauri::command]
pub fn settings_open(app: AppHandle) -> double_love_engine::OperationResult<()> {
    match open_settings_window(&app) {
        Ok(()) => double_love_engine::OperationResult::success(()),
        Err(error) => {
            double_love_engine::OperationResult::failed("SETTINGS_WINDOW_FAILED", error.to_string())
        }
    }
}
