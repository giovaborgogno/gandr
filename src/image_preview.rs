//! Image preview support (M15, see ADR 0008).
//!
//! For now this module owns image *metadata* probing only — format + pixel
//! dimensions, read from the file header without a full decode. It's what the
//! preview placeholder shows (`PNG · 800×600 · 42.1 KB`) while the actual
//! terminal rendering (via `ratatui-image`) is built in M15b.
//!
//! It's a leaf utility (depends only on the `image` crate) so the `diff`,
//! `browser`, and `ui` modules can all use it without depending on each other —
//! consistent with the module direction in `docs/architecture.md`.

use std::io::Cursor;
use std::path::Path;

/// Compressed images larger than this are not probed/previewed (the byte-count
/// placeholder shows instead). Bounds work on a pathological or mislabeled
/// input. Separate from the diff engine's `MAX_DIFF_BYTES` (which only decides
/// "no inline text diff"). Note this bounds the *compressed* input, not the
/// decoded pixel buffer — that's [`MAX_DECODE_ALLOC`].
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

/// Cap on the memory a single decode may allocate (the decoded RGBA buffer, not
/// the compressed input). A small file can declare huge dimensions (a
/// decompression bomb); the `image` crate aborts the decode past this ceiling
/// (→ `None` → placeholder) instead of spiking to its ~512 MiB default. 256 MiB
/// still admits any realistic screenshot/photo (~64 MP).
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;

/// Lightweight metadata about a raster image, shown in the preview placeholder
/// (and, from M15b, used to size the rendered image to its pane).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// Human format label, e.g. `"PNG"`, `"JPEG"`.
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
}

impl ImageInfo {
    /// A one-line summary: `PNG · 800×600 · 42.1 KB`.
    pub fn summary(&self) -> String {
        format!(
            "{} · {}×{} · {}",
            self.format,
            self.width,
            self.height,
            human_bytes(self.byte_len)
        )
    }
}

/// Whether `path`'s extension is a raster format we can preview. SVG is handled
/// separately, behind a feature flag (M15d), so it's intentionally excluded here.
pub fn is_image_path(path: &Path) -> bool {
    matches!(
        ext_lower(path).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

fn ext_lower(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Probe format + dimensions from raw bytes, reading only the header (no full
/// decode). Returns `None` if the bytes are empty, exceed [`MAX_IMAGE_BYTES`],
/// or aren't a decodable image of a supported format.
pub fn probe(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let format = reader.format()?;
    let (width, height) = reader.into_dimensions().ok()?;
    Some(ImageInfo {
        format: format_name(format),
        width,
        height,
        byte_len: bytes.len(),
    })
}

/// Fully decode raster bytes into an image ready for terminal rendering (M15b).
/// Returns `None` for empty input, input over [`MAX_IMAGE_BYTES`], or bytes that
/// aren't a decodable image of a supported format. This is the expensive step
/// (proportional to pixel count), so callers run it off the UI thread.
pub fn decode(bytes: &[u8]) -> Option<image::DynamicImage> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    // Bound the decoded-buffer allocation (decompression-bomb guard). `Limits` is
    // `#[non_exhaustive]`, so it must be built by mutating a default.
    #[allow(clippy::field_reassign_with_default)]
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    reader.decode().ok()
}

fn format_name(f: image::ImageFormat) -> &'static str {
    use image::ImageFormat::*;
    match f {
        Png => "PNG",
        Jpeg => "JPEG",
        Gif => "GIF",
        WebP => "WebP",
        Bmp => "BMP",
        _ => "image",
    }
}

/// Format a byte count compactly (`42.1 KB`, `3.4 MB`, `900 B`).
fn human_bytes(n: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let f = n as f64;
    if f < KB {
        return format!("{n} B");
    }
    // Pick the unit by the *rounded* magnitude so a value like 1023.97 KB shows
    // as "1.0 MB", not "1024.0 KB".
    let kb = f / KB;
    if kb >= 1023.95 {
        format!("{:.1} MB", f / MB)
    } else {
        format!("{kb:.1} KB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Encode a solid-color image of the given size to in-memory bytes.
    fn encode(w: u32, h: u32, fmt: image::ImageFormat) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 40, 40, 255]));
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, fmt)
            .expect("encode");
        buf.into_inner()
    }

    #[test]
    fn detects_image_extensions_case_insensitively() {
        assert!(is_image_path(&PathBuf::from("a/b/logo.PNG")));
        assert!(is_image_path(&PathBuf::from("shot.jpeg")));
        assert!(is_image_path(&PathBuf::from("anim.gif")));
        assert!(!is_image_path(&PathBuf::from("main.rs")));
        assert!(!is_image_path(&PathBuf::from("diagram.svg"))); // SVG excluded (M15d)
        assert!(!is_image_path(&PathBuf::from("noext")));
    }

    #[test]
    fn probes_dimensions_and_format_without_full_decode() {
        let png = encode(3, 2, image::ImageFormat::Png);
        let info = probe(&png).expect("png probe");
        assert_eq!(info.format, "PNG");
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.byte_len, png.len());

        let bmp = encode(5, 4, image::ImageFormat::Bmp);
        let info = probe(&bmp).expect("bmp probe");
        assert_eq!(info.format, "BMP");
        assert_eq!((info.width, info.height), (5, 4));
    }

    #[test]
    fn rejects_non_images_and_bounds() {
        assert_eq!(probe(b""), None);
        assert_eq!(probe(b"not an image, just text bytes"), None);
        // Over the cap: a valid header but too many bytes → refused.
        let mut huge = encode(2, 2, image::ImageFormat::Png);
        huge.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(probe(&huge), None);
    }

    #[test]
    fn summary_is_human_readable() {
        let info = ImageInfo {
            format: "PNG",
            width: 800,
            height: 600,
            byte_len: 43_100,
        };
        assert_eq!(info.summary(), "PNG · 800×600 · 42.1 KB");
    }
}
