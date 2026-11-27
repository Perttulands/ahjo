//! Images under one folder, for an assistant to look at what an agent made:
//! screenshots and rendered output. A folder is named explicitly and must be
//! under a root; an image is named from it and may not leave it, through
//! `..` or a link ([`Roots::in_folder`]). The listing walks as a search does
//! ([`super::find`]): no links, no hidden or build folders below the one
//! named. Reading checks the bytes, not the name, and gives only PNG, JPEG
//! and WebP within [`MAX_IMAGE_BYTES`] and [`MAX_IMAGE_SIDE`].

use std::fs;
use std::io::Read;
use std::path::Path;

use ahjo_core::Error;
use ahjo_files_contract::commands::{
    FileImage, FileImages, ImageData, ImageEntry, MAX_IMAGE_BYTES, MAX_IMAGE_SIDE,
};
use ahjo_files_contract::{NOT_IMAGE, OUTSIDE_ROOTS, TOO_LARGE};
use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::error::FilesError;
use super::roots::Roots;

/// How many images a listing answers unless asked for another number.
const LISTED: usize = 50;
/// The most a listing answers.
const MOST_LISTED: usize = 200;
/// Entries a listing looks at before it stops and says it is truncated.
const VISIT_LIMIT: usize = 100_000;

/// The type a file's name says it is, if it is an image this gives.
fn named_type(name: &str) -> Option<&'static str> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub fn list(
    roots: &Roots,
    node: &str,
    folder: &str,
    limit: Option<usize>,
) -> Result<FileImages, Error> {
    let limit = limit.unwrap_or(LISTED);
    if !(1..=MOST_LISTED).contains(&limit) {
        return Err(Error::new(
            "bad_request",
            format!("limit is from 1 to {MOST_LISTED}"),
        ));
    }
    let root = roots.existing(folder)?;
    if !root.is_dir() {
        return Err(FilesError::NotFolder.into());
    }
    let mounts = super::find::mount_points();
    let mut images = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    // A failure to read the named folder is an error; an unreadable folder
    // below it is skipped.
    let mut pending = vec![(
        root.clone(),
        Some(fs::read_dir(&root).map_err(FilesError::from)?),
    )];
    'walk: while let Some((dir, opened)) = pending.pop() {
        let entries = match opened.map_or_else(|| fs::read_dir(&dir), Ok) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > VISIT_LIMIT {
                truncated = true;
                break 'walk;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if !super::find::skipped(&name) && !mounts.contains(&path) {
                    pending.push((path, None));
                }
                continue;
            }
            // A link is not followed, so a file listed is one reading can
            // reach.
            let (true, Some(mime_type)) = (kind.is_file(), named_type(&name)) else {
                continue;
            };
            let (Ok(meta), Ok(relative)) = (entry.metadata(), path.strip_prefix(&root)) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            images.push(ImageEntry {
                path: relative.replace('\\', "/"),
                mime_type: mime_type.to_owned(),
                size: meta.len(),
                modified: super::disk::modified(&meta),
                too_large: meta.len() > MAX_IMAGE_BYTES,
            });
        }
    }
    images.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    if images.len() > limit {
        images.truncate(limit);
        truncated = true;
    }
    Ok(FileImages {
        node: node.to_owned(),
        folder: text(&root)?,
        images,
        truncated,
    })
}

pub fn read(roots: &Roots, node: &str, folder: &str, relative: &str) -> Result<FileImage, Error> {
    let named = folder;
    let folder = roots.existing(named)?;
    // The folder is under a root, so outside now means the path left it.
    let file = roots
        .in_folder(named, relative)
        .map_err(|error| match error {
            FilesError::Outside => Error::new(
                OUTSIDE_ROOTS,
                format!(
                    "{relative} leads out of {}; name an image as file_list_images lists it",
                    folder.display()
                ),
            ),
            other => other.into(),
        })?;
    let opened = fs::File::open(&file).map_err(FilesError::from)?;
    let meta = opened.metadata().map_err(FilesError::from)?;
    if !meta.is_file() {
        return Err(FilesError::NotFile.into());
    }
    let too_large = |size: String| {
        Error::new(
            TOO_LARGE,
            format!(
                "{relative} is {size}; the most an image is given at is {} MiB",
                MAX_IMAGE_BYTES >> 20
            ),
        )
    };
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(too_large(format!("{} bytes", meta.len())));
    }
    // Read no more than the limit, whatever the file grew to since.
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    opened
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(FilesError::from)?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(too_large("over the limit".to_owned()));
    }
    let Some((mime_type, width, height)) = sniff(&bytes) else {
        return Err(Error::new(
            NOT_IMAGE,
            format!("{relative} is not a PNG, JPEG or WebP image"),
        ));
    };
    if width.max(height) > MAX_IMAGE_SIDE || width == 0 || height == 0 {
        return Err(Error::new(
            TOO_LARGE,
            format!(
                "{relative} is {width}×{height} pixels; an image is given at up to \
                 {MAX_IMAGE_SIDE} pixels a side"
            ),
        ));
    }
    Ok(FileImage {
        node: node.to_owned(),
        folder: text(&folder)?,
        path: file
            .strip_prefix(&folder)
            .ok()
            .and_then(Path::to_str)
            .map(|path| path.replace('\\', "/"))
            .ok_or(FilesError::Outside)?,
        mime_type: mime_type.to_owned(),
        width,
        height,
        size: bytes.len() as u64,
        modified: super::disk::modified(&meta),
        version: super::disk::version(&bytes),
        image: ImageData {
            mime_type: mime_type.to_owned(),
            data: STANDARD.encode(&bytes),
        },
    })
}

fn text(path: &Path) -> Result<String, Error> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::new("bad_request", "path is not UTF-8"))
}

/// What the bytes are, by their header: the type and the size in pixels.
fn sniff(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    let be32 = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let le16 = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let le24 = |at: usize| {
        let b = bytes.get(at..at + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16)? == b"IHDR" {
        return Some(("image/png", be32(16)?, be32(20)?));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12)? == b"WEBP" {
        let (width, height) = match bytes.get(12..16)? {
            b"VP8 " if bytes.get(23..26)? == b"\x9d\x01\x2a" => {
                (u32::from(le16(26)? & 0x3fff), u32::from(le16(28)? & 0x3fff))
            }
            b"VP8L" if *bytes.get(20)? == 0x2f => {
                let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
                (1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff))
            }
            b"VP8X" => (1 + le24(24)?, 1 + le24(27)?),
            _ => return None,
        };
        return Some(("image/webp", width, height));
    }
    if bytes.starts_with(b"\xff\xd8") {
        let (width, height) = jpeg_size(bytes)?;
        return Some(("image/jpeg", width, height));
    }
    None
}

/// A JPEG's size, from its first start-of-frame segment.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        // Markers may be padded with any number of 0xff.
        while *bytes.get(at)? == 0xff && *bytes.get(at + 1)? == 0xff {
            at += 1;
        }
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        at += 2;
        match marker {
            // Standalone markers carry no length.
            0x01 | 0xd0..=0xd7 => continue,
            0xd8..=0xda => return None,
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
        if length < 2 {
            return None;
        }
        // Every SOFn but DHT (c4), JPG (c8) and DAC (cc).
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let height = u16::from_be_bytes([*bytes.get(at + 3)?, *bytes.get(at + 4)?]);
            let width = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            return Some((u32::from(width), u32::from(height)));
        }
        at += length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 PNG, stretched in its header to the size asked for.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend(b"\x08\x02\0\0\0");
        bytes
    }

    #[test]
    fn the_bytes_say_what_an_image_is_and_how_large() {
        assert_eq!(sniff(&png(640, 480)), Some(("image/png", 640, 480)));
        // A JPEG with an APP0 segment before its baseline frame.
        let mut jpeg = b"\xff\xd8\xff\xe0\0\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0".to_vec();
        jpeg.extend(b"\xff\xc0\0\x11\x08\x01\xe0\x02\x80\x03");
        assert_eq!(sniff(&jpeg), Some(("image/jpeg", 640, 480)));
        let mut lossless = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        let bits: u32 = (640 - 1) | ((480 - 1) << 14);
        lossless.extend(bits.to_le_bytes());
        assert_eq!(sniff(&lossless), Some(("image/webp", 640, 480)));
        let mut extended = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        extended.extend([0x7f, 0x02, 0x00, 0xdf, 0x01, 0x00]);
        assert_eq!(sniff(&extended), Some(("image/webp", 640, 480)));
        let mut lossy = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0\x9d\x01\x2a".to_vec();
        lossy.extend([0x80, 0x02, 0xe0, 0x01]);
        assert_eq!(sniff(&lossy), Some(("image/webp", 640, 480)));
        assert_eq!(sniff(b"GIF89a\x01\0\x01\0"), None);
        assert_eq!(sniff(b"<svg/>"), None);
        assert_eq!(sniff(b"\xff\xd8\xff"), None, "a cut-off JPEG");
    }
}
