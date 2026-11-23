// Save / save-as file operations. Owner: crop/export teammate.
//
// Parity source: src/js/core/export.js (`suggestedName`, `saveDoc`) and the web
// save/save-as flow. Saving always writes freshly encoded bytes, so EXIF/GPS
// from the source file can never leak into the output: the image crate decoder
// only returns pixels and the encoders emit a bare JPEG/PNG/WebP stream.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::doc::Doc;
use crate::state::OutputFormat;

/// `<stem>-edit.<ext>` next to the original (export.js `suggestedName`).
pub fn suggested_name(doc: &Doc, format: OutputFormat) -> PathBuf {
    let file = suggested_file_name(doc, format);
    match doc.path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(file),
        _ => PathBuf::from(file),
    }
}

/// Just the file name part of the save-as suggestion.
pub fn suggested_file_name(doc: &Doc, format: OutputFormat) -> String {
    let name = if doc.name.is_empty() {
        doc.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("image")
            .to_string()
    } else {
        doc.name.clone()
    };
    let stem = strip_extension(&name);
    let stem = if stem.is_empty() { "image" } else { stem };
    format!("{stem}-edit.{}", format.ext())
}

fn strip_extension(name: &str) -> &str {
    // Mirrors the web regex /\.[^./\\]+$/ — only the last extension is dropped.
    match name.rfind('.') {
        Some(i) if i > 0 && !name[i + 1..].contains(['/', '\\']) => &name[..i],
        _ => name,
    }
}

fn temp_path_for(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image");
    let tmp = format!(".{name}.tmp-{}-{n}", std::process::id());
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(tmp),
        _ => PathBuf::from(tmp),
    }
}

/// Write bytes to `path`, replacing it atomically (write a sibling temp file
/// first, then rename) so a crash mid-write cannot truncate the original.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = temp_path_for(path);
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("寫入檔案失敗：{err}"));
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("寫入檔案失敗：{err}"));
    }
    Ok(())
}

/// Overwrite the document in place (檔案 → 儲存).
pub fn overwrite(doc: &Doc, bytes: &[u8]) -> Result<(), String> {
    write(&doc.path, bytes)
}

/// 另存新檔: write to `path` and report the path that was actually used.
pub fn save_as(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    write(path, bytes)?;
    Ok(path.to_path_buf())
}

/// Human-readable file size for the status bar (e.g. "1.4 MB").
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Doc, LoadOptions};
    use crate::export;
    use image::{Rgba, RgbaImage};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-save-tests-{}-{tag}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
        })
    }

    /// Insert a fake APP1/Exif segment right after SOI so the source looks like
    /// a camera JPEG for the purposes of this test.
    fn jpeg_with_exif(payload: &[u8]) -> Vec<u8> {
        let base = export::encode(&gradient(64, 48), crate::state::OutputFormat::Jpeg, 90);
        let mut out = Vec::with_capacity(base.len() + payload.len() + 8);
        out.extend_from_slice(&base[..2]); // SOI
        let seg_len = (payload.len() + 2) as u16;
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&seg_len.to_be_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&base[2..]);
        out
    }

    fn doc_from(path: &Path) -> Doc {
        Doc::load(path, LoadOptions { max_pixels: 256 }).expect("load doc")
    }

    /// Write PNG bytes to a path with any extension (the doc loader sniffs the
    /// content, so a `.jpg` path is fine for suggestion-name tests).
    fn write_png(path: &Path, w: u32, h: u32) {
        let bytes = export::encode(&gradient(w, h), crate::state::OutputFormat::Png, 90);
        std::fs::write(path, bytes).expect("write test png");
    }

    #[test]
    fn suggested_name_matches_the_web_pattern() {
        let dir = temp_dir("name");
        let path = dir.join("IMG_1234.jpg");
        write_png(&path, 8, 8);
        let doc = doc_from(&path);
        assert_eq!(
            suggested_name(&doc, OutputFormat::Jpeg),
            dir.join("IMG_1234-edit.jpg")
        );
        assert_eq!(
            suggested_name(&doc, OutputFormat::Png),
            dir.join("IMG_1234-edit.png")
        );
        assert_eq!(
            suggested_name(&doc, OutputFormat::Webp),
            dir.join("IMG_1234-edit.webp")
        );
        assert_eq!(suggested_file_name(&doc, OutputFormat::Jpeg), "IMG_1234-edit.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggested_name_keeps_multi_dot_stems() {
        let dir = temp_dir("dots");
        let path = dir.join("holiday.2024.png");
        write_png(&path, 8, 8);
        let doc = doc_from(&path);
        assert_eq!(
            suggested_name(&doc, OutputFormat::Png),
            dir.join("holiday.2024-edit.png")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_creates_and_overwrites() {
        let dir = temp_dir("write");
        let path = dir.join("out.png");
        write(&path, b"first-payload").expect("write");
        assert_eq!(std::fs::read(&path).unwrap(), b"first-payload");
        // shorter payload must fully replace the old contents
        write(&path, b"second").expect("overwrite");
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        // no temp files left behind
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overwrite_replaces_the_document_in_place() {
        let dir = temp_dir("inplace");
        let path = dir.join("doc.png");
        write_png(&path, 10, 10);
        let doc = doc_from(&path);
        overwrite(&doc, b"new-bytes").expect("overwrite");
        assert_eq!(std::fs::read(&path).unwrap(), b"new-bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_as_returns_the_written_path() {
        let dir = temp_dir("saveas");
        let path = dir.join("copy.jpg");
        let bytes = export::encode(&gradient(16, 12), crate::state::OutputFormat::Jpeg, 80);
        let written = save_as(&path, &bytes).expect("save as");
        assert_eq!(written, path);
        assert_eq!(std::fs::read(&path).unwrap().len(), bytes.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exif_and_gps_are_not_carried_into_saved_output() {
        let dir = temp_dir("exif");
        let source = dir.join("camera.jpg");
        // Fake EXIF + GPS payload with the standard "Exif\0\0" TIFF header.
        let mut payload = b"Exif\0\0II*\0\x08\0\0\0GPS-AND-CAMERA-DATA".to_vec();
        payload.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        let bytes = jpeg_with_exif(&payload);
        std::fs::write(&source, &bytes).expect("write source jpeg");

        // Sanity: the source really carries an APP1 Exif segment.
        assert!(
            bytes.windows(6).any(|w| w == b"Exif\0\0"),
            "test setup failed to inject Exif"
        );

        let doc = doc_from(&source);
        let flat = export::flatten(&doc, &[], None, None, None);
        let out = export::encode(&flat.image, crate::state::OutputFormat::Jpeg, 90);
        assert!(
            !out.windows(6).any(|w| w == b"Exif\0\0"),
            "saved JPEG still contains Exif"
        );
        let mut i = 2;
        while i + 4 <= out.len() {
            if out[i] != 0xFF {
                break;
            }
            let marker = out[i + 1];
            if marker == 0xDA || marker == 0xD9 {
                break;
            }
            assert_ne!(marker, 0xE1, "saved JPEG has an APP1 segment");
            let len = u16::from_be_bytes([out[i + 2], out[i + 3]]) as usize;
            if len < 2 {
                break;
            }
            i += 2 + len;
        }

        // Round trip through save.rs: bytes on disk are exactly what we encoded.
        let target = suggested_name(&doc, crate::state::OutputFormat::Jpeg);
        write(&target, &out).expect("write output");
        assert_eq!(std::fs::read(&target).unwrap(), out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_size_reads_like_the_status_bar() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(1024 * 1024 * 3 / 2), "1.5 MB");
    }

    #[test]
    fn strip_extension_edges() {
        assert_eq!(strip_extension("a.jpg"), "a");
        assert_eq!(strip_extension("a.tar.gz"), "a.tar");
        assert_eq!(strip_extension("noext"), "noext");
        assert_eq!(strip_extension(".hidden"), ".hidden");
        assert_eq!(strip_extension("a."), "a");
    }
}
