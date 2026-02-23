//! This module implements Home-specific matchers for the `infer`
//! crate, allowing us to check for magic bytes that only exist
//! in the context of PlayStation Home development.

use hdk_archive::structs::{ArchiveVersion, Endianness};

/// Convenience function to convert a magic value to an Endianness enum.
pub const fn magic_to_endianess(buf: &[u8; 4]) -> Endianness {
    match buf {
        b"\xE1\x17\xEF\xAD" => Endianness::Little,
        b"\xAD\xEF\x17\xE1" => Endianness::Big,
        _ => panic!("Invalid magic value"),
    }
}

/// Archive matcher based on the magic value in the header.
///
/// Archives can be either big-endian or little-endian, so we check for both.
fn archive_matcher(buf: &[u8]) -> bool {
    if buf.len() < 4 {
        return false;
    }

    use hdk_archive::structs::ARCHIVE_MAGIC;

    let magic = &buf[0..4];
    magic == ARCHIVE_MAGIC.to_le_bytes() || magic == ARCHIVE_MAGIC.to_be_bytes()
}

/// Convenience function to extract the archive version from the header bytes, if it matches the archive magic.
fn extract_version(buf: &[u8]) -> Option<ArchiveVersion> {
    if buf.len() < 8 {
        return None;
    }

    use hdk_archive::structs::ARCHIVE_MAGIC;

    let endianess = if buf[0..4] == ARCHIVE_MAGIC.to_le_bytes() {
        Endianness::Little
    } else if buf[0..4] == ARCHIVE_MAGIC.to_be_bytes() {
        Endianness::Big
    } else {
        return None;
    };

    let version_and_flags: u32 = match endianess {
        Endianness::Little => u32::from_le_bytes(buf[4..8].try_into().unwrap()),
        Endianness::Big => u32::from_be_bytes(buf[4..8].try_into().unwrap()),
    };

    let version: u16 = (version_and_flags >> 16) as u16;
    ArchiveVersion::try_from(version).ok()
}

/// SHARC archive matcher based on the magic value in the header.
fn sharc_matcher(buf: &[u8]) -> bool {
    if buf.len() < 8 {
        return false;
    }

    let magic = &buf[0..4];

    if !archive_matcher(magic) {
        return false;
    }

    if let Some(version) = extract_version(magic) {
        return version == ArchiveVersion::SHARC;
    }

    false
}

/// BAR archive matcher based on the magic value in the header.
fn bar_matcher(buf: &[u8]) -> bool {
    if buf.len() < 8 {
        return false;
    }

    let magic = &buf[0..4];

    if !archive_matcher(magic) {
        return false;
    }

    if let Some(version) = extract_version(magic) {
        return version == ArchiveVersion::BAR;
    }

    false
}

/// EdgeLZMA segmented compression matcher
fn edge_lzma_matcher(buf: &[u8]) -> bool {
    if buf.len() < 4 {
        return false;
    }

    &buf[0..4] == hdk_comp::lzma::SEGMENT_MAGIC
}

/// SDAT container matcher
fn sdat_matcher(buf: &[u8]) -> bool {
    // SDAT files have "NPD" at the start and "SDATA" within the last 32 bytes.
    if buf.len() < 36 {
        return false;
    }

    // SDAT files start with "NPD" in ASCII
    let magic_start = &buf[0..3] == b"NPD";

    // Get the last 32 bytes and check if `SDATA` appears in it
    let magic_end = buf[buf.len() - 32..]
        .windows(5)
        .any(|window| window == b"SDATA");

    magic_start && magic_end
}

// Type alias to represent MIME types
pub type MimeType = (&'static str, &'static str);

pub const MIME_SHARC: MimeType = ("hdk-sharc", "application/x-hdk-sharc");
pub const MIME_BAR: MimeType = ("hdk-bar", "application/x-hdk-bar");
pub const MIME_ARCHIVE: MimeType = ("hdk-archive", "application/x-hdk-archive");
pub const MIME_EDGE_LZMA: MimeType = ("hdk-edge-lzma", "application/x-hdk-edge-lzma");
pub const MIME_SDAT: MimeType = ("hdk-sdat", "application/x-hdk-sdat");

/// Return a well-formed Infer matcher
pub fn get_matcher() -> infer::Infer {
    let mut matcher = infer::Infer::new();

    // Archive matchers
    //
    // Matchers are checked in order, so more specific ones should be added first.
    // Since SHARC and BAR are both types of archive, we check for them before the more general archive matcher.
    matcher.add(MIME_SHARC.0, MIME_SHARC.1, sharc_matcher);
    matcher.add(MIME_BAR.0, MIME_BAR.1, bar_matcher);
    matcher.add(MIME_ARCHIVE.0, MIME_ARCHIVE.1, archive_matcher);

    // Compression matchers
    // Note: EdgeZlib does not have a magic value, so we can't reliably detect it.
    //       Thankfully, they don't appear often "in the wild" and are really only used
    //       inside of archives, so we can get away with not having a reliable matcher for it.
    matcher.add(MIME_EDGE_LZMA.0, MIME_EDGE_LZMA.1, edge_lzma_matcher);

    // Sony SDAT matcher
    matcher.add(MIME_SDAT.0, MIME_SDAT.1, sdat_matcher);

    matcher
}
