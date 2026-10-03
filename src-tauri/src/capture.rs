use std::path::PathBuf;

use chrono::Utc;
use clipboard_rs::{
    common::RustImage, Clipboard, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, WatcherShutdown,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::store::{self, ImageItem, SavedImage, Store};

pub(crate) struct AppState {
    pub(crate) store: std::sync::Mutex<Store>,
    pub(crate) clipboard: std::sync::Mutex<ClipboardContext>,
}

struct ClipboardWatch {
    app: AppHandle,
}

impl ClipboardHandler for ClipboardWatch {
    fn on_clipboard_change(&mut self) {
        if let Err(error) = capture_clipboard(&self.app) {
            eprintln!("클립보드를 저장하지 못했습니다: {error}");
        }
    }
}

pub fn start_watcher(app: AppHandle) -> Result<WatcherShutdown, String> {
    let mut watcher = ClipboardWatcherContext::new().map_err(|error| error.to_string())?;
    let shutdown = watcher
        .add_handler(ClipboardWatch { app })
        .get_shutdown_channel();

    std::thread::Builder::new()
        .name("clipnest-clipboard".into())
        .spawn(move || watcher.start_watch())
        .map_err(|error| error.to_string())?;

    Ok(shutdown)
}

pub fn copy_stored_image(app: &AppHandle, id: &str) -> Result<(), String> {
    let path = {
        let state = app.state::<AppState>();
        let store = state.store.lock().map_err(|error| error.to_string())?;
        if !store::valid_id(id) || !store.contains(id) {
            return Err("보관 중인 이미지가 아닙니다".into());
        }
        store.image_path(id)
    };

    let path = path
        .to_str()
        .ok_or("이미지 경로를 읽지 못했습니다")?
        .to_string();
    let image = clipboard_rs::RustImageData::from_path(&path).map_err(|error| error.to_string())?;

    let state = app.state::<AppState>();
    let clipboard = state.clipboard.lock().map_err(|error| error.to_string())?;
    clipboard
        .set_image(image)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn capture_clipboard(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let images_dir = {
        let store = state.store.lock().map_err(|error| error.to_string())?;
        store.images_dir()
    };

    let (files, image) = {
        let clipboard = state.clipboard.lock().map_err(|error| error.to_string())?;
        let files = if clipboard.has(ContentFormat::Files) {
            clipboard.get_files().unwrap_or_default()
        } else {
            Vec::new()
        };
        let image = if clipboard.has(ContentFormat::Image) {
            clipboard.get_image().ok().filter(|image| !image.is_empty())
        } else {
            None
        };
        (files, image)
    };

    // 이 앱에 저장해 둔 파일을 다시 복사한 경우에는 미리보기 이미지도 저장하지 않는다.
    if files.iter().any(|file| is_stored_file(file, &images_dir)) {
        return Ok(());
    }

    let mut handled_file = false;
    for file in files {
        if !store::is_image_path(&normalize_clipboard_path(&file)) {
            continue;
        }
        match save_file(app, &file) {
            Ok(()) => handled_file = true,
            Err(error) => eprintln!("이미지 파일을 저장하지 못했습니다: {error}"),
        }
    }
    if handled_file {
        return Ok(());
    }

    if let Some(image) = image {
        save_clipboard_image(app, &image)?;
    }
    Ok(())
}

fn save_file(app: &AppHandle, raw_path: &str) -> Result<(), String> {
    let path = normalize_clipboard_path(raw_path);
    let path = path
        .to_str()
        .ok_or("이미지 경로를 읽지 못했습니다")?
        .to_string();
    let image = clipboard_rs::RustImageData::from_path(&path).map_err(|error| error.to_string())?;
    save_clipboard_image(app, &image)
}

fn save_clipboard_image(
    app: &AppHandle,
    image: &clipboard_rs::RustImageData,
) -> Result<(), String> {
    if image.is_empty() {
        return Ok(());
    }

    let rgba = image.to_rgba8().map_err(|error| error.to_string())?;
    if rgba.width() == 0 || rgba.height() == 0 {
        return Ok(());
    }

    let id = store::content_id(rgba.width(), rgba.height(), rgba.as_raw());
    let png = image.to_png().map_err(|error| error.to_string())?;
    let saved = SavedImage {
        id,
        width: rgba.width(),
        height: rgba.height(),
        created_at: Utc::now().to_rfc3339(),
        bytes: png.get_bytes().len() as u64,
    };

    let state = app.state::<AppState>();
    let item = {
        let mut store = state.store.lock().map_err(|error| error.to_string())?;
        store.insert(saved, png.get_bytes())?
    };

    if let Some(item) = item {
        emit_added(app, item);
    }
    Ok(())
}

fn emit_added(app: &AppHandle, item: ImageItem) {
    if let Err(error) = app.emit("image-added", item) {
        eprintln!("화면 알림을 보내지 못했습니다: {error}");
    }
}

fn is_stored_file(raw_path: &str, images_dir: &std::path::Path) -> bool {
    store::is_inside_dir(&normalize_clipboard_path(raw_path), images_dir)
}

pub fn normalize_clipboard_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"');
    if let Some(rest) = trimmed.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        if let Some(drive_path) = rest.strip_prefix('/') {
            if is_windows_drive_path(drive_path) {
                return PathBuf::from(drive_path.replace('/', "\\"));
            }
        }
        return PathBuf::from(rest.replace('/', "\\"));
    }
    PathBuf::from(trimmed)
}

fn is_windows_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::normalize_clipboard_path;

    #[test]
    fn 파일_주소와_윈도우_경로를_같은_경로로_본다() {
        assert_eq!(
            normalize_clipboard_path("file:///C:/Users/clip/photo.png"),
            normalize_clipboard_path(r"C:\Users\clip\photo.png")
        );
    }
}
