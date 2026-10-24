# Third-party notices

s4-xero-extract's own code is licensed under GPL-3.0-only (see [LICENSE](LICENSE)).
It is built with the Rust packages below, at the versions pinned in `Cargo.lock`.
Their licences apply to their own code, separately from the project's licence.
Where a package offers alternative licences, a GPLv3-compatible alternative
applies; for MIT/Apache-2.0 alternatives, either is compatible.

The full licence texts shipped by each package are kept in
[`licenses/`](licenses), one folder per package (`licenses/<name>-<version>/`).

The list covers the Windows x64 build (`cargo metadata --filter-platform
x86_64-pc-windows-msvc`). Builds for other platforms pull in further packages
(for example the Linux file-dialog backend of `rfd`); check `Cargo.lock` and
`cargo metadata` for those.

| Package | Version | Declared licence | Source |
|---|---|---|---|
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/stream-ciphers |
| cipher | 0.5.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| either | 1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hybrid-array |
| inout | 0.2.5 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| log | 0.4.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | https://github.com/rust-windowing/raw-window-handle |
| rayon | 1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rfd | 0.17.2 | MIT | https://github.com/PolyMeilex/rfd |
| snap | 1.1.2 | BSD-3-Clause | https://github.com/BurntSushi/rust-snappy |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |

The Windows executable also contains the Rust standard library (MIT OR Apache-2.0,
https://github.com/rust-lang/rust).
