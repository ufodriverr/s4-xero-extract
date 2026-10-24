//! End-to-end run on a small synthetic install built inside the test.

use std::fs;
use std::path::{Path, PathBuf};

use chacha20::ChaCha20;
use chacha20::cipher::{KeyIvInit, StreamCipher};
use s4_xero_extract::archive::{self, ArchiveKind, MAGIC};
use s4_xero_extract::extract::{self, DECODED_DIR, RECOVERED_DIR};
use s4_xero_extract::scene;

fn push_compact(out: &mut Vec<u8>, v: u64) {
    out.push(4);
    out.extend_from_slice(&(v as u32).to_le_bytes());
}

fn build_archive(kind: ArchiveKind, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    push_compact(&mut out, files.len() as u64);
    for (path, plain) in files {
        let mut obf = plain.to_vec();
        archive::apply_xor(&mut obf, archive::key_for(path.as_bytes()), kind);
        let stored = snap::raw::Encoder::new().compress_vec(&obf).unwrap();
        out.push(1);
        push_compact(&mut out, path.len() as u64);
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&crc32fast::hash(plain).to_le_bytes());
        push_compact(&mut out, plain.len() as u64);
        push_compact(&mut out, stored.len() as u64);
        out.extend_from_slice(&stored);
    }
    out
}

/// A fresh folder under the cargo target tmp dir (build output only).
fn fresh_dir(name: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if base.exists() {
        fs::remove_dir_all(&base).unwrap();
    }
    fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn synthetic_install_end_to_end() {
    let base = fresh_dir("synthetic_install");
    let install_root = base.join("install");
    let out = base.join("out");
    fs::create_dir_all(install_root.join("package")).unwrap();
    fs::write(install_root.join("xerogame.exe"), b"").unwrap();

    // A CC01 scene stored the way the client stores it.
    let mut header = scene::SIGNATURE.to_vec();
    header.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    let body: Vec<u8> = (0..200u32).map(|i| (i ^ 0x5a) as u8).collect();
    let mut nonce: [u8; 12] = header.clone().try_into().unwrap();
    nonce[1] = 0;
    let mut enc = body.clone();
    ChaCha20::new(&(*scene::KEY).into(), &nonce.into()).apply_keystream(&mut enc);
    let cc01: Vec<u8> = header.iter().chain(&enc).copied().collect();
    let plain_scene = vec![1u8, 0, 0, 0, 9, 9, 9, 9, 9];

    let pkg0 = build_archive(
        ArchiveKind::Package,
        &[("xml/a.xml", b"<a>abcdefgh</a>"), ("model/character/cc.scn", &cc01), ("old.txt", b"old")],
    );
    let pkg10 = build_archive(
        ArchiveKind::Package,
        &[
            ("model\\weapon\\plain.scn", &plain_scene),
            ("OLD.txt", b"newer"),
            ("../escape.txt", b"nope"),
        ],
    );
    let launcher = build_archive(ArchiveKind::Launcher, &[("xeroclient.index", &[7u8; 16])]);
    fs::write(install_root.join("package").join("0.xs"), &pkg0).unwrap();
    fs::write(install_root.join("package").join("10.xs"), &pkg10).unwrap();
    fs::write(install_root.join("xero.xs"), &launcher).unwrap();

    let install = extract::locate_install(&install_root).unwrap();
    assert_eq!(install.archives.len(), 3);
    assert_eq!(install.archives[2].kind, ArchiveKind::Launcher);
    assert!(extract::check_output_location(&install_root, &install_root.join("x")).is_err());
    assert!(extract::check_output_location(&install_root, &install_root).is_err());
    extract::check_output_location(&install_root, &out).unwrap();

    let plan = extract::plan(&install, &[]).unwrap();
    assert_eq!(plan.total_entries, 7);
    assert_eq!(plan.jobs.len(), 5);
    assert_eq!(plan.overridden, vec!["old.txt".to_string()]);
    assert_eq!(plan.rejected.len(), 1);
    assert_eq!(plan.rejected[0].0, "../escape.txt");

    let report = extract::run(&install, &plan, &out, &|_, _, _| {}).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.written, 5);
    assert_eq!(report.decoded_written, 1);

    let rec = out.join(RECOVERED_DIR);
    let dec = out.join(DECODED_DIR);
    assert_eq!(fs::read(rec.join("xml/a.xml")).unwrap(), b"<a>abcdefgh</a>");
    assert_eq!(fs::read(rec.join("model/character/cc.scn")).unwrap(), cc01);
    assert_eq!(fs::read(rec.join("model/weapon/plain.scn")).unwrap(), plain_scene);
    assert_eq!(fs::read(rec.join("OLD.txt")).unwrap(), b"newer");
    assert_eq!(fs::read(rec.join("xeroclient.index")).unwrap(), vec![7u8; 16]);
    assert!(!base.join("escape.txt").exists() && !out.join("escape.txt").exists());
    let decoded = fs::read(dec.join("model/character/cc.scn")).unwrap();
    assert_eq!(&decoded[..12], &nonce);
    assert_eq!(&decoded[12..], body.as_slice());
    assert!(!dec.join("model/weapon/plain.scn").exists());
    assert!(!dec.join("xml/a.xml").exists());

    // Second run: everything present with the right size, nothing rewritten.
    let again = extract::run(&install, &plan, &out, &|_, _, _| {}).unwrap();
    assert_eq!((again.written, again.skipped, again.decoded_written), (0, 5, 0));

    // A truncated output file is rewritten.
    fs::write(rec.join("xml/a.xml"), b"<a>").unwrap();
    let fix = extract::run(&install, &plan, &out, &|_, _, _| {}).unwrap();
    assert_eq!(fix.written, 1);
    assert_eq!(fs::read(rec.join("xml/a.xml")).unwrap(), b"<a>abcdefgh</a>");

    // A missing decoded scene is regenerated from the recovered copy.
    fs::remove_file(dec.join("model/character/cc.scn")).unwrap();
    let third = extract::run(&install, &plan, &out, &|_, _, _| {}).unwrap();
    assert_eq!((third.written, third.decoded_written), (0, 1));
    assert_eq!(fs::read(dec.join("model/character/cc.scn")).unwrap(), decoded);

    // --only limits the plan.
    let only = extract::plan(&install, &[extract::normalise_prefix("Model/")]).unwrap();
    assert_eq!(only.jobs.len(), 2);
}

#[test]
fn rejects_folder_that_is_not_an_install() {
    let base = fresh_dir("not_an_install");
    let err = extract::locate_install(&base).unwrap_err().to_string();
    assert!(err.contains("xerogame.exe"), "{err}");
}
