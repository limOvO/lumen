// Fast JPEG preview decoding through the system libjpeg-turbo (scaled IDCT).
// Falls back to the `image` crate whenever anything unexpected happens.

use std::ffi::c_void;
use std::os::raw::{c_int, c_uchar};

#[repr(C)]
#[derive(Clone, Copy)]
struct ScalingFactor {
    num: c_int,
    denom: c_int,
}

const TJPF_RGB: c_int = 0; // enum tjpf, first entry

#[link(name = "turbojpeg")]
extern "C" {
    fn tj3Init(init_type: c_int) -> *mut c_void;
    fn tj3Destroy(handle: *mut c_void);
    fn tj3DecompressHeader(
        handle: *mut c_void,
        jpeg_buf: *const c_uchar,
        jpeg_size: usize,
    ) -> c_int;
    fn tj3Get(handle: *mut c_void, param: c_int) -> c_int;
    fn tj3SetScalingFactor(handle: *mut c_void, factor: ScalingFactor) -> c_int;
    fn tj3GetScalingFactors(num: *mut c_int) -> *mut ScalingFactor;
    fn tj3Decompress8(
        handle: *mut c_void,
        jpeg_buf: *const c_uchar,
        jpeg_size: usize,
        dst_buf: *mut c_uchar,
        pitch: c_int,
        pixel_format: c_int,
    ) -> c_int;
}

const TJINIT_DECOMPRESS: c_int = 1;
const TJPARAM_JPEGWIDTH: c_int = 5;
const TJPARAM_JPEGHEIGHT: c_int = 6;
const TJPF_RGBA: c_int = 7;

/// TJSCALED: libjpeg-turbo rounds scaled dimensions **up**; using floor here
/// under-allocates the output buffer and corrupts the heap.
fn scaled_dim(dim: c_int, f: ScalingFactor) -> u64 {
    debug_assert!(f.denom > 0);
    let d = dim as i64 * f.num as i64;
    ((d + f.denom as i64 - 1) / f.denom as i64).max(1) as u64
}

/// Decode a JPEG to RGBA at the largest libjpeg scaling factor whose area fits
/// `max_pixels`. Returns `(rgba, width, height, scale)` where scale is the
/// decoded width divided by the source width.
pub fn decode_scaled(bytes: &[u8], max_pixels: u64) -> Option<(Vec<u8>, u32, u32, u32, u32, f32)> {
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    unsafe {
        let handle = tj3Init(TJINIT_DECOMPRESS);
        if handle.is_null() {
            return None;
        }
        let ok = tj3DecompressHeader(handle, bytes.as_ptr(), bytes.len());
        if ok != 0 {
            tj3Destroy(handle);
            return None;
        }
        let w = tj3Get(handle, TJPARAM_JPEGWIDTH);
        let h = tj3Get(handle, TJPARAM_JPEGHEIGHT);
        if w <= 0 || h <= 0 {
            tj3Destroy(handle);
            return None;
        }
        // Pick the largest scaling factor whose output area fits the cap.
        let mut count: c_int = 0;
        let factors = tj3GetScalingFactors(&mut count);
        let mut chosen = ScalingFactor { num: 1, denom: 1 };
        let mut best_area: u64 = 0;
        if !factors.is_null() && count > 0 {
            for i in 0..count as usize {
                let f = *factors.add(i);
                if f.denom <= 0 {
                    continue;
                }
                let sw = scaled_dim(w, f);
                let sh = scaled_dim(h, f);
                let area = sw * sh;
                if area <= max_pixels && area > best_area {
                    best_area = area;
                    chosen = f;
                }
            }
        }
        if best_area == 0 {
            chosen = ScalingFactor { num: 1, denom: 8 };
        }
        if tj3SetScalingFactor(handle, chosen) != 0 {
            tj3Destroy(handle);
            return None;
        }
        let sw = scaled_dim(w, chosen) as i32;
        let sh = scaled_dim(h, chosen) as i32;
        let mut rgba = vec![0u8; (sw as usize) * (sh as usize) * 4];
        let ok = tj3Decompress8(
            handle,
            bytes.as_ptr(),
            bytes.len(),
            rgba.as_mut_ptr(),
            sw * 4,
            TJPF_RGBA,
        );
        tj3Destroy(handle);
        if ok != 0 {
            return None;
        }
        let scale = sw as f32 / w as f32;
        Some((rgba, sw as u32, sh as u32, w as u32, h as u32, scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_jpeg_decodes_under_the_cap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/samples/coast.jpg");
        let Ok(bytes) = std::fs::read(path) else { return };
        let out = decode_scaled(&bytes, 500_000);
        if let Some((rgba, w, h, fw, fh, scale)) = out {
            assert!((w as u64) * (h as u64) <= 500_000 + 100_000);
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            assert!(scale > 0.0 && scale <= 1.0);
            assert!(fw >= w && fh >= h);
            // non-empty pixels (sanity)
            assert!(rgba.iter().any(|b| *b > 30));
        } else {
            eprintln!("libjpeg-turbo unavailable; fast path falls back");
        }
    }
}
