//! Finds the embedded EXIF thumbnail of a JPEG file (IFD1, JPEGInterchangeFormat) and its size,
//! from the file header only. This decides THM-03 without a decode of the main image.

use std::io::Read;
use std::path::Path;

/// Bytes read from the file start. APP1 (EXIF) is at most 64 KB and comes first in practice.
const HEAD: u64 = 128 * 1024;

pub struct EmbeddedThumb {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn u16_at(b: &[u8], i: usize, le: bool) -> Option<u16> {
    let s: [u8; 2] = b.get(i..i + 2)?.try_into().ok()?;
    Some(if le { u16::from_le_bytes(s) } else { u16::from_be_bytes(s) })
}

fn u32_at(b: &[u8], i: usize, le: bool) -> Option<u32> {
    let s: [u8; 4] = b.get(i..i + 4)?.try_into().ok()?;
    Some(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
}

/// Returns the TIFF block of the APP1 Exif segment.
fn exif_tiff(head: &[u8]) -> Option<&[u8]> {
    if head.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 4 <= head.len() {
        if head[i] != 0xFF {
            return None;
        }
        let marker = head[i + 1];
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = u16_at(head, i + 2, false)? as usize;
        if marker == 0xDA || marker == 0xD9 {
            return None; // start of scan: no EXIF before the image data
        }
        let seg = head.get(i + 4..i + 2 + len)?;
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return Some(&seg[6..]);
        }
        i += 2 + len;
    }
    None
}

/// Reads the size from the SOF marker of a JPEG stream.
pub fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 9 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            i += 2;
            continue;
        }
        let len = u16_at(b, i + 2, false)? as usize;
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u16_at(b, i + 5, false)? as u32;
            let w = u16_at(b, i + 7, false)? as u32;
            return Some((w, h));
        }
        i += 2 + len;
    }
    None
}

pub fn find_in(head: &[u8]) -> Option<EmbeddedThumb> {
    let tiff = exif_tiff(head)?;
    let le = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let ifd0 = u32_at(tiff, 4, le)? as usize;
    let n0 = u16_at(tiff, ifd0, le)? as usize;
    let ifd1 = u32_at(tiff, ifd0 + 2 + n0 * 12, le)? as usize;
    if ifd1 == 0 {
        return None;
    }
    let n1 = u16_at(tiff, ifd1, le)? as usize;
    let (mut off, mut len) = (None, None);
    for k in 0..n1 {
        let e = ifd1 + 2 + k * 12;
        match u16_at(tiff, e, le)? {
            0x0201 => off = Some(u32_at(tiff, e + 8, le)? as usize),
            0x0202 => len = Some(u32_at(tiff, e + 8, le)? as usize),
            _ => {}
        }
    }
    let (off, len) = (off?, len?);
    let bytes = tiff.get(off..off.checked_add(len)?)?.to_vec();
    let (width, height) = jpeg_size(&bytes)?;
    Some(EmbeddedThumb { bytes, width, height })
}

pub fn find(path: &Path) -> Option<EmbeddedThumb> {
    let f = std::fs::File::open(path).ok()?;
    let mut head = Vec::with_capacity(HEAD as usize);
    f.take(HEAD).read_to_end(&mut head).ok()?;
    find_in(&head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_iphone_exif_thumbnail() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/example gallery/IMG_2439.JPG");
        if !p.exists() {
            return;
        }
        let t = find(&p).expect("EXIF thumbnail");
        assert!(t.width >= 120 && t.height >= 90, "{}x{}", t.width, t.height);
        assert!(t.width <= 320);
    }

    #[test]
    fn rejects_non_jpeg() {
        assert!(find_in(b"\x89PNG....").is_none());
        assert!(find_in(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]).is_none());
    }
}
