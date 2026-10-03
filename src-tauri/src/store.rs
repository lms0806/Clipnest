use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedImage {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub created_at: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageItem {
    pub id: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub created_at: String,
    pub bytes: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct IndexFile {
    images: Vec<SavedImage>,
}

pub struct Store {
    root: PathBuf,
    images: Vec<SavedImage>,
}

impl Store {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        let images_dir = root.join("images");
        fs::create_dir_all(&images_dir).map_err(|error| error.to_string())?;

        let index_path = root.join("index.json");
        let mut images = match fs::read(&index_path) {
            Ok(data) => serde_json::from_slice::<IndexFile>(&data)
                .map(|file| file.images)
                .unwrap_or_else(|error| {
                    eprintln!("목록 파일을 읽지 못했습니다: {error}");
                    Vec::new()
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.to_string()),
        };

        images.retain(|image| {
            valid_id(&image.id) && images_dir.join(format!("{}.png", image.id)).is_file()
        });
        images.sort_by(|left, right| right.created_at.cmp(&left.created_at));

        Ok(Self { root, images })
    }

    pub fn list(&self) -> Vec<ImageItem> {
        self.images
            .iter()
            .map(|image| self.to_item(image))
            .collect()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.images.iter().any(|image| image.id == id)
    }

    pub fn images_dir(&self) -> PathBuf {
        self.root.join("images")
    }

    pub fn image_path(&self, id: &str) -> PathBuf {
        self.images_dir().join(format!("{id}.png"))
    }

    pub fn insert(&mut self, image: SavedImage, png: &[u8]) -> Result<Option<ImageItem>, String> {
        if !valid_id(&image.id) {
            return Err("이미지 식별자가 올바르지 않습니다".into());
        }
        if self.contains(&image.id) {
            return Ok(None);
        }

        let path = self.image_path(&image.id);
        fs::write(&path, png).map_err(|error| error.to_string())?;
        self.images.insert(0, image);
        if let Err(error) = self.write_index() {
            let failed = self.images.remove(0);
            let _ = fs::remove_file(self.image_path(&failed.id));
            return Err(error);
        }

        Ok(Some(self.to_item(&self.images[0])))
    }

    pub fn remove(&mut self, id: &str) -> Result<bool, String> {
        if !valid_id(id) {
            return Err("잘못된 이미지입니다".into());
        }

        let Some(position) = self.images.iter().position(|image| image.id == id) else {
            return Ok(false);
        };

        let path = self.image_path(id);
        if path.exists() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
        self.images.remove(position);
        self.write_index()?;
        Ok(true)
    }

    fn to_item(&self, image: &SavedImage) -> ImageItem {
        ImageItem {
            id: image.id.clone(),
            path: self.image_path(&image.id).to_string_lossy().into_owned(),
            width: image.width,
            height: image.height,
            created_at: image.created_at.clone(),
            bytes: image.bytes,
        }
    }

    fn write_index(&self) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(&IndexFile {
            images: self.images.clone(),
        })
        .map_err(|error| error.to_string())?;
        fs::write(self.root.join("index.json"), data).map_err(|error| error.to_string())
    }
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn content_id(width: u32, height: u32, pixels: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(width.to_le_bytes());
    hasher.update(height.to_le_bytes());
    hasher.update(pixels);
    hex_encode(&hasher.finalize())
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension.as_str()))
}

pub fn is_inside_dir(path: &Path, dir: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    path.starts_with(dir)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0xf) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("시각")
            .as_nanos();
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("clipnest-{stamp}-{seq}"))
    }

    fn sample(id: &str, created_at: &str) -> SavedImage {
        SavedImage {
            id: id.to_string(),
            width: 1,
            height: 1,
            created_at: created_at.to_string(),
            bytes: 1,
        }
    }

    #[test]
    fn 같은_픽셀은_같은_식별자다() {
        let pixels = [1, 2, 3, 4];
        assert_eq!(content_id(1, 1, &pixels), content_id(1, 1, &pixels));
        assert_ne!(content_id(1, 1, &pixels), content_id(1, 1, &[9, 2, 3, 4]));
    }

    #[test]
    fn png로_다시_읽어도_식별자가_유지된다() {
        let image = image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("이미지");
        let id = content_id(image.width(), image.height(), image.as_raw());
        let encoded = {
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgba8(image)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("인코딩");
            bytes
        };
        let decoded = image::load_from_memory(&encoded)
            .expect("디코딩")
            .to_rgba8();
        assert_eq!(
            id,
            content_id(decoded.width(), decoded.height(), decoded.as_raw())
        );
    }

    #[test]
    fn 같은_이미지는_한_번만_저장한다() {
        let root = temp_root();
        let mut store = Store::open(root.clone()).expect("저장소");
        let id = "ab".repeat(32);
        let first = store
            .insert(sample(&id, "2026-10-03T00:00:00Z"), b"png")
            .expect("저장");
        let second = store
            .insert(sample(&id, "2026-10-03T00:00:01Z"), b"png")
            .expect("중복 저장");

        assert!(first.is_some());
        assert!(second.is_none());
        assert_eq!(store.list().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn 최신_항목이_앞에_온다() {
        let root = temp_root();
        let mut store = Store::open(root.clone()).expect("저장소");
        let older = "aa".repeat(32);
        let newer = "bb".repeat(32);
        store
            .insert(sample(&older, "2026-10-03T00:00:00Z"), b"old")
            .expect("저장");
        store
            .insert(sample(&newer, "2026-10-03T00:00:01Z"), b"new")
            .expect("저장");

        let list = store.list();
        assert_eq!(list[0].id, newer);
        assert_eq!(list[1].id, older);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn 삭제하면_파일과_목록에서_빠진다() {
        let root = temp_root();
        let mut store = Store::open(root.clone()).expect("저장소");
        let id = "cd".repeat(32);
        store
            .insert(sample(&id, "2026-10-03T00:00:00Z"), b"png")
            .expect("저장");
        assert!(store.remove(&id).expect("삭제"));
        assert!(store.list().is_empty());
        assert!(!store.image_path(&id).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn 저장_폴더_안의_파일만_보관_파일로_본다() {
        let root = temp_root();
        let store = Store::open(root.clone()).expect("저장소");
        let inside = store.images_dir().join("inside.png");
        fs::write(&inside, b"png").expect("파일");
        let outside_dir = root.join("outside");
        fs::create_dir_all(&outside_dir).expect("폴더");
        let outside = outside_dir.join("outside.png");
        fs::write(&outside, b"png").expect("파일");

        assert!(is_inside_dir(&inside, &store.images_dir()));
        assert!(!is_inside_dir(&outside, &store.images_dir()));
        assert!(is_image_path(Path::new("Photo.PNG")));
        assert!(!is_image_path(Path::new("note.txt")));
        let _ = fs::remove_dir_all(root);
    }
}
