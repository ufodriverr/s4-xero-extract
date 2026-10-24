//! "CC01" scene files.
//!
//! Some `.scn` resources start with the u32 `0x0000cc01` followed by the
//! scene magic `0x6278d57a` and carry an encrypted body:
//!
//! * The first 12 bytes are clear. With byte 1 set to 0 they form both the
//!   output header and the 12-byte ChaCha20 nonce.
//! * Bytes 12.. are ChaCha20 (20 rounds, 96-bit nonce, block counter 0,
//!   keystream starting at offset 0) with a fixed 32-byte key.
//!
//! The decoded file is the normalised 12-byte header followed by the
//! decrypted body, so it has the same length as the source.

use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};

/// 32-byte ChaCha20 key used for CC01 scene bodies (ASCII, no terminator).
pub const KEY: &[u8; 32] = b"nevergonnagiveyouupuwuiloveuwus!";

/// First 8 bytes of a CC01 scene: u32 `0x0000cc01`, then u32 `0x6278d57a`.
pub const SIGNATURE: [u8; 8] = [0x01, 0xcc, 0x00, 0x00, 0x7a, 0xd5, 0x78, 0x62];

/// Length of the clear header that doubles as the nonce.
pub const HEADER_LEN: usize = 12;

/// True when `head` (at least the first 8 bytes of a file) carries the CC01
/// signature.
pub fn has_signature(head: &[u8]) -> bool {
    head.len() >= SIGNATURE.len() && head[..SIGNATURE.len()] == SIGNATURE
}

/// Decodes a CC01 scene. Returns `None` if `data` is not a CC01 scene or is
/// too short to hold the 12-byte header.
pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    if !has_signature(data) || data.len() < HEADER_LEN {
        return None;
    }
    let mut out = data.to_vec();
    out[1] = 0;
    let nonce: [u8; HEADER_LEN] = out[..HEADER_LEN].try_into().expect("12-byte header");
    let mut cipher = ChaCha20::new(&(*KEY).into(), &nonce.into());
    cipher.apply_keystream(&mut out[HEADER_LEN..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// RFC 8439 appendix A.1, test vectors 1 and 2: all-zero key and nonce,
    /// keystream blocks for counter 0 and counter 1.
    #[test]
    fn chacha20_rfc8439_keystream() {
        let mut buf = [0u8; 128];
        let mut c = ChaCha20::new(&[0u8; 32].into(), &[0u8; 12].into());
        c.apply_keystream(&mut buf);
        let expected = hex(concat!(
            "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7",
            "da41597c5157488d7724e03fb8d84a376a43b8f41518a11cc387b669b2ee6586",
            "9f07e7be5551387a98ba977c732d080dcb0f29a048e3656912c6533e32ee7aed",
            "29b721769ce64e43d57133b074d839d531ed1f28510afb45ace10a1f4b794d6f",
        ));
        assert_eq!(buf.as_slice(), expected.as_slice());
    }

    #[test]
    fn decode_roundtrip() {
        let mut header = SIGNATURE.to_vec();
        header.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
        let body: Vec<u8> = (0..300u32).map(|i| (i * 7) as u8).collect();

        // Encrypt the way the client's files are laid out: the nonce is the
        // header with byte 1 cleared, the stored header keeps byte 1.
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&header);
        nonce[1] = 0;
        let mut enc = body.clone();
        ChaCha20::new(&(*KEY).into(), &nonce.into()).apply_keystream(&mut enc);
        let mut source = header.clone();
        source.extend_from_slice(&enc);

        let decoded = decode(&source).unwrap();
        assert_eq!(decoded.len(), source.len());
        assert_eq!(&decoded[..12], &nonce);
        assert_eq!(&decoded[12..], body.as_slice());
    }

    #[test]
    fn non_cc01_is_ignored() {
        assert!(decode(&[1, 0, 0, 0, 0x7a, 0xd5, 0x78, 0x62, 0, 0, 0, 0]).is_none());
        assert!(decode(&SIGNATURE).is_none()); // too short for the header
        assert!(!has_signature(&[0x01, 0xcc]));
    }
}
