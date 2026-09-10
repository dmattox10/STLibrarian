use crate::core::paths::{thumb_dir, thumb_path};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::collections::HashSet;

/// A rendered preview is a 512px PNG; anything much past this is not a
/// thumbnail, it is a client trying to fill the disk.
const MAX_THUMB_BYTES: usize = 4 * 1024 * 1024;

/// The hash becomes a filename, and on the Pro server it arrives over HTTP.
/// Anything but lowercase hex is refused, so `../../etc/passwd` never reaches
/// the filesystem.
pub fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Store a `data:image/png;base64,...` URL as a PNG on disk.
pub fn save(hash: &str, data_url: &str) -> Result<(), String> {
    if !is_valid_hash(hash) {
        return Err("Invalid model hash".into());
    }
    let encoded = data_url
        .split_once("base64,")
        .map(|(_, rest)| rest)
        .unwrap_or(data_url);

    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|e| format!("Thumbnail was not valid base64: {}", e))?;

    if bytes.is_empty() {
        return Err("Thumbnail was empty".into());
    }
    if bytes.len() > MAX_THUMB_BYTES {
        return Err("Thumbnail too large".into());
    }
    // A PNG always starts with this signature. Checking it keeps arbitrary
    // uploads from being served back out later under an image content type.
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err("Thumbnail was not a PNG".into());
    }

    std::fs::create_dir_all(thumb_dir()).map_err(|e| e.to_string())?;
    std::fs::write(thumb_path(hash), bytes).map_err(|e| e.to_string())
}

pub fn read_png(hash: &str) -> Option<Vec<u8>> {
    if !is_valid_hash(hash) {
        return None;
    }
    std::fs::read(thumb_path(hash)).ok()
}

pub fn load_data_url(hash: &str) -> Option<String> {
    let bytes = read_png(hash)?;
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

pub fn exists(hash: &str) -> bool {
    is_valid_hash(hash) && thumb_path(hash).exists()
}

/// Drop previews whose model is no longer in the library. Thumbnails are keyed
/// by content hash, so editing a model orphans its old preview rather than
/// corrupting it -- this is the sweep that reclaims the space.
pub fn prune(live_hashes: &HashSet<String>) -> usize {
    let dir = match std::fs::read_dir(thumb_dir()) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let mut removed = 0;
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !live_hashes.contains(&stem) && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_64_char_lowercase_hex_is_accepted() {
        let good = "a".repeat(64);
        assert!(is_valid_hash(&good));
        assert!(is_valid_hash(
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        ));

        assert!(!is_valid_hash(""));
        assert!(!is_valid_hash(&"a".repeat(63)));
        assert!(!is_valid_hash(&"a".repeat(65)));
        assert!(!is_valid_hash(&"z".repeat(64)));
    }

    /// The reason the check exists: these must never become a path.
    #[test]
    fn traversal_attempts_are_refused() {
        for attempt in [
            "../../../../etc/passwd",
            "..",
            "/etc/passwd",
            "a/../../b",
            &format!("{}/../evil", "a".repeat(58)),
        ] {
            assert!(!is_valid_hash(attempt), "{} should be refused", attempt);
            assert!(save(attempt, "data:image/png;base64,").is_err());
            assert!(read_png(attempt).is_none());
            assert!(!exists(attempt));
        }
    }

    #[test]
    fn non_png_payloads_are_refused() {
        let hash = "c".repeat(64);
        // Valid base64, but a GIF header.
        let gif = STANDARD.encode(b"GIF89a-not-really-an-image");
        let err = save(&hash, &format!("data:image/png;base64,{}", gif)).unwrap_err();
        assert!(err.contains("not a PNG"), "got: {}", err);

        assert!(save(&hash, "data:image/png;base64,!!!not base64!!!").is_err());
        assert!(save(&hash, "data:image/png;base64,").is_err());
    }
}
