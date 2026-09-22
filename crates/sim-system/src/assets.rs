//! Content-addressed reference images next to a system document.
//! Images are copied to `<document>.assets/<hash>.<ext>` and never decoded
//! here beyond their header, so the core stays free of image codecs.
use crate::document::Asset;
use crate::SystemError;
use std::path::Path;

/// Width and height from a PNG or JPEG header.
pub fn image_size(bytes: &[u8]) -> Result<(u32, u32, &'static str, &'static str), SystemError> {
    if bytes.len() >= 24 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        return Ok((w, h, "image/png", "png"));
    }
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) || marker == 0xFF {
                i += if marker == 0xFF { 1 } else { 2 };
                continue;
            }
            let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            // Start-of-frame markers carry the dimensions (not DHT/JPG/DAC).
            if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Ok((w, h, "image/jpeg", "jpg"));
            }
            i += 2 + length;
        }
    }
    Err(SystemError::Invalid("reference images must be PNG or JPEG".into()))
}

/// Copy an image beside the document. Returns the asset key and record.
pub fn import_image(document_path: &Path, source: &Path) -> Result<(String, Asset), SystemError> {
    let bytes = std::fs::read(source)?;
    let (width_px, height_px, media_type, extension) = image_size(&bytes)?;
    if width_px == 0 || height_px == 0 {
        return Err(SystemError::Invalid("image has no pixels".into()));
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let directory = assets_directory(document_path);
    std::fs::create_dir_all(&directory)?;
    let file = format!("{}.{extension}", &hash[..32]);
    let target = directory.join(&file);
    if !target.exists() {
        crate::store::write_atomic(&target, &bytes)?;
    }
    let relative = format!("{}/{file}", directory.file_name().unwrap().to_string_lossy());
    let id = hash[..16].to_string();
    Ok((
        id,
        Asset {
            path: relative,
            media_type: media_type.into(),
            width_px,
            height_px,
            bytes: bytes.len() as u64,
            original_name: source.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        },
    ))
}

pub fn assets_directory(document_path: &Path) -> std::path::PathBuf {
    let name = document_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "system".into());
    let stem = name.strip_suffix(".system.json").or_else(|| name.strip_suffix(".json")).unwrap_or(&name).to_string();
    document_path.with_file_name(format!("{stem}.assets"))
}

/// Absolute path of an asset recorded in a document at `document_path`.
pub fn resolve(document_path: &Path, asset: &Asset) -> std::path::PathBuf {
    document_path.parent().unwrap_or(Path::new(".")).join(&asset.path)
}
