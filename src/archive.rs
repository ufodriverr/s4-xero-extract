//! XS resource archives.
//!
//! Layout (all integers little-endian):
//!
//! | Field              | Encoding                                       |
//! |--------------------|------------------------------------------------|
//! | magic              | `f2 1b 42 eb`                                  |
//! | entry count        | compact uint                                   |
//! | per entry:         |                                                |
//! |   marker           | one byte, always `01`                          |
//! |   path length      | compact uint                                   |
//! |   path             | that many ASCII bytes, no terminator           |
//! |   plaintext CRC32  | 4 bytes                                        |
//! |   original length  | compact uint                                   |
//! |   stored length    | compact uint                                   |
//! |   payload          | stored-length bytes, raw Snappy block          |
//!
//! A compact uint is one width byte (1, 2, 4 or 8) followed by that many
//! bytes of little-endian integer.
//!
//! The decompressed payload is obfuscated by XOR with an 8-byte key derived
//! from the stored path (see [`key_for`]). Only whole 8-byte blocks are
//! XORed; the tail rule differs between the package archives and the
//! launcher archive (see [`ArchiveKind`]).

use std::io::{self, Read, Seek};

/// Four-byte signature at the start of every archive.
pub const MAGIC: [u8; 4] = [0xf2, 0x1b, 0x42, 0xeb];

/// Constant mixed into the FNV-1a-64 path hash to form the XOR key.
const KEY_MIX: u64 = 0x372f_044c;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Upper bound for a stored path, to reject garbage early.
const MAX_PATH_LEN: u64 = 4096;

/// Which tail rule the XOR layer of an archive follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    /// `package/<n>.xs`: every complete 8-byte block is XORed, the final
    /// 0..=7 bytes are left as they are.
    Package,
    /// `xero.xs` in the install root: blocks strictly before the end are
    /// XORed, the final 1..=8 bytes are left as they are (so a complete final
    /// block stays untouched when the length is a multiple of 8).
    Launcher,
}

/// One entry of an archive index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path exactly as stored (ASCII). This is the key-derivation input.
    pub path: String,
    /// CRC32 of the decoded plaintext.
    pub crc32: u32,
    /// Plaintext length in bytes.
    pub original_len: u64,
    /// Stored (compressed) payload length in bytes.
    pub stored_len: u64,
    /// Absolute offset of the payload inside the archive file.
    pub payload_offset: u64,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn read_u8<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

/// Reads a compact uint: width byte (1, 2, 4 or 8) then that many LE bytes.
pub fn read_compact<R: Read>(r: &mut R) -> io::Result<u64> {
    let width = read_u8(r)? as usize;
    if !matches!(width, 1 | 2 | 4 | 8) {
        return Err(invalid(format!("unsupported integer width {width}")));
    }
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf[..width])?;
    Ok(u64::from_le_bytes(buf))
}

/// Parses the whole index of an archive. `file_len` is the archive size; the
/// last payload must end exactly at the end of the file.
pub fn parse_index<R: Read + Seek>(r: &mut R, file_len: u64) -> io::Result<Vec<Entry>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(invalid("not an XS archive (bad magic)"));
    }
    let count = read_compact(r)?;
    // Every entry needs at least 1+2+1+4+2+2 bytes, which bounds the count.
    if count > file_len / 12 + 1 {
        return Err(invalid(format!("implausible entry count {count}")));
    }
    let mut entries = Vec::with_capacity(count as usize);
    for index in 0..count {
        let ctx = |e: io::Error| io::Error::new(e.kind(), format!("entry {index}: {e}"));
        let marker = read_u8(r).map_err(ctx)?;
        if marker != 0x01 {
            return Err(invalid(format!("entry {index}: unexpected marker {marker:#04x}")));
        }
        let path_len = read_compact(r).map_err(ctx)?;
        if path_len == 0 || path_len > MAX_PATH_LEN {
            return Err(invalid(format!("entry {index}: implausible path length {path_len}")));
        }
        let mut raw = vec![0u8; path_len as usize];
        r.read_exact(&mut raw).map_err(ctx)?;
        if !raw.is_ascii() {
            return Err(invalid(format!("entry {index}: path is not ASCII")));
        }
        let path = String::from_utf8(raw).expect("ASCII is valid UTF-8");
        let mut crc = [0u8; 4];
        r.read_exact(&mut crc).map_err(ctx)?;
        let original_len = read_compact(r).map_err(ctx)?;
        let stored_len = read_compact(r).map_err(ctx)?;
        let payload_offset = r.stream_position()?;
        let end = payload_offset
            .checked_add(stored_len)
            .filter(|&end| end <= file_len)
            .ok_or_else(|| invalid(format!("entry {index} ({path}): payload runs past end of archive")))?;
        r.seek(io::SeekFrom::Start(end))?;
        entries.push(Entry {
            path,
            crc32: u32::from_le_bytes(crc),
            original_len,
            stored_len,
            payload_offset,
        });
    }
    let end = r.stream_position()?;
    if end != file_len {
        return Err(invalid(format!(
            "{} unexpected trailing bytes after the last entry",
            file_len - end
        )));
    }
    Ok(entries)
}

/// FNV-1a, 64-bit.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(FNV_OFFSET, |h, &b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME))
}

/// XOR key for a resource: FNV-1a-64 of the exact stored path bytes, XOR
/// `0x372f044c`. Applied as 8 little-endian bytes, which is the same as
/// XORing each block read as a little-endian u64 with this value.
pub fn key_for(path: &[u8]) -> u64 {
    fnv1a64(path) ^ KEY_MIX
}

/// Number of leading bytes covered by the XOR layer for a plaintext of `len`.
pub fn xor_end(len: usize, kind: ArchiveKind) -> usize {
    let full = len / 8 * 8;
    match kind {
        ArchiveKind::Package => full,
        ArchiveKind::Launcher if len > 0 && len % 8 == 0 => len - 8,
        ArchiveKind::Launcher => full,
    }
}

/// Applies (or removes; XOR is its own inverse) the block XOR layer in place.
pub fn apply_xor(data: &mut [u8], key: u64, kind: ArchiveKind) {
    let end = xor_end(data.len(), kind);
    for block in data[..end].chunks_exact_mut(8) {
        let v = u64::from_le_bytes(block.try_into().expect("8-byte chunk")) ^ key;
        block.copy_from_slice(&v.to_le_bytes());
    }
}

/// Decodes a stored payload: raw Snappy decompression, length check, XOR
/// layer, CRC32 check. Returns the plaintext.
pub fn decode_payload(entry: &Entry, kind: ArchiveKind, stored: &[u8]) -> io::Result<Vec<u8>> {
    let declared = snap::raw::decompress_len(stored)
        .map_err(|e| invalid(format!("bad Snappy header: {e}")))?;
    if declared as u64 != entry.original_len {
        return Err(invalid(format!(
            "decompressed length {declared} does not match the index ({})",
            entry.original_len
        )));
    }
    let mut data = snap::raw::Decoder::new()
        .decompress_vec(stored)
        .map_err(|e| invalid(format!("Snappy decompression failed: {e}")))?;
    if data.len() as u64 != entry.original_len {
        return Err(invalid(format!(
            "decompressed {} bytes, index says {}",
            data.len(),
            entry.original_len
        )));
    }
    apply_xor(&mut data, key_for(entry.path.as_bytes()), kind);
    let crc = crc32fast::hash(&data);
    if crc != entry.crc32 {
        return Err(invalid(format!(
            "CRC32 mismatch (expected {:08x}, got {crc:08x})",
            entry.crc32
        )));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn push_compact(out: &mut Vec<u8>, v: u64, width: u8) {
        out.push(width);
        out.extend_from_slice(&v.to_le_bytes()[..width as usize]);
    }

    /// Builds an archive in memory the same way the game's packer lays it out.
    fn build_archive(kind: ArchiveKind, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        push_compact(&mut out, files.len() as u64, 4);
        for (i, (path, plain)) in files.iter().enumerate() {
            let mut obf = plain.to_vec();
            apply_xor(&mut obf, key_for(path.as_bytes()), kind);
            let stored = snap::raw::Encoder::new().compress_vec(&obf).unwrap();
            out.push(1);
            // Mix widths to exercise the compact uint reader.
            let w = [1u8, 2, 4, 8][i % 4];
            push_compact(&mut out, path.len() as u64, w.max(1));
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(&crc32fast::hash(plain).to_le_bytes());
            push_compact(&mut out, plain.len() as u64, 8);
            push_compact(&mut out, stored.len() as u64, 4);
            out.extend_from_slice(&stored);
        }
        out
    }

    #[test]
    fn fnv1a64_known_values() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn key_is_hash_xor_constant_as_le_bytes() {
        let path = b"xml/item.xml";
        let key = key_for(path);
        assert_eq!(key, fnv1a64(path) ^ 0x372f_044c);
        // Byte-wise application must equal u64 application.
        let mut a = [0u8; 8];
        apply_xor(&mut a, key, ArchiveKind::Package);
        assert_eq!(a, key.to_le_bytes());
    }

    #[test]
    fn package_tail_rule() {
        assert_eq!(xor_end(0, ArchiveKind::Package), 0);
        assert_eq!(xor_end(7, ArchiveKind::Package), 0);
        assert_eq!(xor_end(8, ArchiveKind::Package), 8);
        assert_eq!(xor_end(15, ArchiveKind::Package), 8);
        assert_eq!(xor_end(16, ArchiveKind::Package), 16);
        let mut d = vec![0u8; 19];
        apply_xor(&mut d, 0x0101_0101_0101_0101, ArchiveKind::Package);
        assert!(d[..16].iter().all(|&b| b == 1));
        assert!(d[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn launcher_tail_rule() {
        assert_eq!(xor_end(0, ArchiveKind::Launcher), 0);
        assert_eq!(xor_end(7, ArchiveKind::Launcher), 0);
        assert_eq!(xor_end(8, ArchiveKind::Launcher), 0);
        assert_eq!(xor_end(9, ArchiveKind::Launcher), 8);
        assert_eq!(xor_end(16, ArchiveKind::Launcher), 8);
        assert_eq!(xor_end(17, ArchiveKind::Launcher), 16);
        let mut d = vec![0u8; 24];
        apply_xor(&mut d, 0x0101_0101_0101_0101, ArchiveKind::Launcher);
        assert!(d[..16].iter().all(|&b| b == 1));
        assert!(d[16..].iter().all(|&b| b == 0));
    }

    #[test]
    fn compact_uint_widths() {
        let mut c = Cursor::new(vec![1, 0x7f, 2, 0x34, 0x12, 4, 1, 0, 0, 0, 8, 1, 0, 0, 0, 0, 0, 0, 0x80]);
        assert_eq!(read_compact(&mut c).unwrap(), 0x7f);
        assert_eq!(read_compact(&mut c).unwrap(), 0x1234);
        assert_eq!(read_compact(&mut c).unwrap(), 1);
        assert_eq!(read_compact(&mut c).unwrap(), 0x8000_0000_0000_0001);
        assert!(read_compact(&mut Cursor::new(vec![3, 0, 0, 0])).is_err());
    }

    #[test]
    fn roundtrip_synthetic_archive_both_kinds() {
        let files: Vec<(&str, Vec<u8>)> = vec![
            ("xml/a.xml", b"<root>hello</root>".to_vec()),
            ("model/x.scn", (0..64u8).collect()),
            ("empty.txt", Vec::new()),
            ("t/seven.bin", vec![9; 7]),
            ("t/big.bin", (0..10_000u32).map(|i| (i * 31 % 251) as u8).collect()),
        ];
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(p, d)| (*p, d.as_slice())).collect();
        for kind in [ArchiveKind::Package, ArchiveKind::Launcher] {
            let bytes = build_archive(kind, &refs);
            let len = bytes.len() as u64;
            let entries = parse_index(&mut Cursor::new(&bytes), len).unwrap();
            assert_eq!(entries.len(), files.len());
            for (entry, (path, plain)) in entries.iter().zip(&files) {
                assert_eq!(entry.path, *path);
                let start = entry.payload_offset as usize;
                let stored = &bytes[start..start + entry.stored_len as usize];
                assert_eq!(&decode_payload(entry, kind, stored).unwrap(), plain);
            }
            // Wrong tail rule must be caught by the CRC (64 bytes is a
            // multiple of 8, so the two rules differ for model/x.scn).
            let other = match kind {
                ArchiveKind::Package => ArchiveKind::Launcher,
                ArchiveKind::Launcher => ArchiveKind::Package,
            };
            let e = &entries[1];
            let stored = &bytes[e.payload_offset as usize..(e.payload_offset + e.stored_len) as usize];
            assert!(decode_payload(e, other, stored).is_err());
        }
    }

    #[test]
    fn rejects_trailing_bytes_and_bad_magic() {
        let mut bytes = build_archive(ArchiveKind::Package, &[("a.txt", b"abc")]);
        bytes.push(0);
        let len = bytes.len() as u64;
        assert!(parse_index(&mut Cursor::new(&bytes), len).is_err());
        let mut bad = build_archive(ArchiveKind::Package, &[("a.txt", b"abc")]);
        bad[0] = 0;
        let len = bad.len() as u64;
        assert!(parse_index(&mut Cursor::new(&bad), len).is_err());
    }

    #[test]
    fn crc_mismatch_is_an_error() {
        let bytes = build_archive(ArchiveKind::Package, &[("a.txt", b"abcdefghijkl")]);
        let len = bytes.len() as u64;
        let mut e = parse_index(&mut Cursor::new(&bytes), len).unwrap().remove(0);
        e.crc32 ^= 1;
        let stored = &bytes[e.payload_offset as usize..];
        assert!(decode_payload(&e, ArchiveKind::Package, stored).is_err());
    }
}
