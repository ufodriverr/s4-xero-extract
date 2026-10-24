//! Extractor for the resource archives of a locally installed S4 League
//! client from the Xero private server.
//!
//! * [`archive`]: the XS archive container and its per-resource XOR layer.
//! * [`scene`]: the encrypted "CC01" scene variant.
//! * [`extract`]: install detection, planning and writing the output tree.

pub mod archive;
pub mod extract;
pub mod scene;
