mod capture;
mod store;

use std::sync::Mutex;

use capture::{copy_stored_image, start_watcher, AppState};
use clipboard_rs::ClipboardContext;
use store::ImageItem;
use tauri::{Emitter, Manager};

#[tauri::command]
fn list_images(state: tauri::State<'_, AppState>) -> Result<Vec<ImageItem>, String> {
    let store = state.store.lock().map_err(|error| error.to_string())?;
    Ok(store.list())
}

#[tauri::command]
fn copy_image(app: tauri::AppHandle, id: String) -> Result<(), String> {
    copy_stored_image(&app, &id)
}

#[tauri::command]
fn delete_image(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let removed = {
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.remove(&id)?
    };
    if removed {
        app.emit("image-removed", id)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let store = store::Store::open(data_dir).map_err(std::io::Error::other)?;
            let clipboard = ClipboardContext::new().map_err(std::io::Error::other)?;
            app.manage(AppState {
                store: Mutex::new(store),
                clipboard: Mutex::new(clipboard),
            });

            let shutdown = start_watcher(app.handle().clone()).map_err(std::io::Error::other)?;
            app.manage(Mutex::new(Some(shutdown)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_images,
            copy_image,
            delete_image
        ])
        .build(tauri::generate_context!())
        .expect("Clipnest를 시작하지 못했습니다")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(control) =
                    app.try_state::<Mutex<Option<clipboard_rs::WatcherShutdown>>>()
                {
                    if let Ok(mut shutdown) = control.lock() {
                        if let Some(shutdown) = shutdown.take() {
                            shutdown.stop();
                        }
                    }
                }
            }
        });
}
