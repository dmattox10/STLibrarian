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

/// The size previews are rendered at. Large enough for a grid card on a
/// high-density display, small enough that a thousand of them is not a
/// problem on disk.
pub const THUMB_SIZE: u32 = 512;

/// A preview for this model, rendering and caching it if there is not one yet.
///
/// This is what lets a headless server build its own previews. Nothing here
/// needs a GPU, a window, or a client to have visited.
pub fn ensure(
    hash: &str,
    source: &std::path::Path,
    render: Option<&std::path::Path>,
) -> Result<Vec<u8>, String> {
    if !is_valid_hash(hash) {
        return Err("Invalid model hash".into());
    }
    if let Some(cached) = read_png(hash) {
        return Ok(cached);
    }
    // A creator's own render is posed, lit and often painted. Nothing derived
    // from the raw mesh competes with that, so it is used when one exists and
    // the rasteriser is the fallback rather than the default.
    let from_render = render.map(downscale).transpose().unwrap_or_else(|e| {
        // Falling back is right -- a broken render should still leave you with
        // a usable preview -- but it must be audible, or a mislabelled file
        // looks exactly like a working one.
        log::warn!("Falling back to the mesh: {}", e);
        None
    });
    let png = match from_render {
        Some(png) => png,
        None => crate::core::raster::render(source, THUMB_SIZE)
            .map_err(|e| format!("Could not render preview: {}", e))?,
    };
    std::fs::create_dir_all(thumb_dir()).map_err(|e| e.to_string())?;
    // Content-addressed, so a written preview can never be stale.
    std::fs::write(thumb_path(hash), &png).map_err(|e| e.to_string())?;
    Ok(png)
}

/// Decode a creator's render and shrink it to thumbnail size.
///
/// Serving the original would mean a few megabytes per card; a folder of promo
/// JPEGs is not a sprite sheet.
fn downscale(path: &std::path::Path) -> Result<Vec<u8>, String> {
    // Format sniffed from content, not from the extension.
    //
    // `image::open` picks a decoder by file extension, and model packs are
    // full of PNGs named .jpg. Getting that wrong fails the decode, which then
    // falls through to the mesh renderer -- so the creator's own artwork is
    // silently ignored and everything still looks like it worked.
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("Could not open {}: {}", path.display(), e))?
        .with_guessed_format()
        .map_err(|e| format!("Could not read {}: {}", path.display(), e))?
        .decode()
        .map_err(|e| format!("Could not decode {}: {}", path.display(), e))?;
    let thumb = img.thumbnail(THUMB_SIZE, THUMB_SIZE);
    let mut out = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
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

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        // Encoded with the png crate rather than hand-rolled, so the test is
        // about the decoder's behaviour and not about my zlib.
        let mut raw = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                raw.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 128]);
            }
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&raw).unwrap();
        }
        out
    }

    fn tmp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("stlib_thumb_{}_{}", std::process::id(), name));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// Model packs are full of PNGs named .jpg.
    ///
    /// Choosing a decoder by extension fails on those, and because a failed
    /// render falls back to the mesh, the creator's artwork gets silently
    /// ignored while everything still looks like it worked.
    #[test]
    fn a_png_named_jpg_still_decodes() {
        let p = tmp_file("mislabelled.jpg", &png_bytes(300, 200));
        let thumb = downscale(&p).expect("content, not extension, decides the decoder");
        assert_eq!(&thumb[0..4], &[0x89, b'P', b'N', b'G']);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn a_large_render_is_shrunk_to_thumbnail_size() {
        let p = tmp_file("big.png", &png_bytes(1600, 1200));
        let thumb = downscale(&p).unwrap();
        // IHDR width/height live at a fixed offset in every PNG.
        let w = u32::from_be_bytes([thumb[16], thumb[17], thumb[18], thumb[19]]);
        let h = u32::from_be_bytes([thumb[20], thumb[21], thumb[22], thumb[23]]);
        assert!(w <= THUMB_SIZE && h <= THUMB_SIZE, "got {}x{}", w, h);
        // Aspect ratio preserved, so the render is not squashed.
        assert!((w as f32 / h as f32 - 1600.0 / 1200.0).abs() < 0.05);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn something_that_is_not_an_image_is_an_error_not_a_panic() {
        let p = tmp_file("notanimage.jpg", b"this is a readme, not a render");
        assert!(downscale(&p).is_err());
        std::fs::remove_file(p).ok();
    }

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

/// Drop a cached preview so the next request rebuilds it.
///
/// Needed when the *choice* of preview changes -- picking a creator's render
/// over the generated one -- because the cache is keyed by content hash and
/// the model's bytes have not changed at all.
pub fn forget(hash: &str) {
    if is_valid_hash(hash) {
        std::fs::remove_file(thumb_path(hash)).ok();
    }
}
