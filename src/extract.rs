//! Locating an install, planning the extraction and writing the output tree.
//!
//! Output layout:
//!
//! * `<out>/recovered/<path>`: every archive resource, decoded.
//! * `<out>/decoded/<path>`: CC01 scenes (see [`crate::scene`]), decrypted.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::archive::{self, ArchiveKind, Entry};
use crate::scene;

/// Sub-folder for every decoded archive resource.
pub const RECOVERED_DIR: &str = "recovered";
/// Sub-folder for decrypted CC01 scenes.
pub const DECODED_DIR: &str = "decoded";

/// One archive file of an install.
#[derive(Clone, Debug)]
pub struct ArchiveFile {
    pub path: PathBuf,
    pub kind: ArchiveKind,
}

/// A validated install folder with its archives in load order.
#[derive(Clone, Debug)]
pub struct Install {
    pub root: PathBuf,
    pub archives: Vec<ArchiveFile>,
}

/// Checks that `root` looks like an S4 League install from the Xero private
/// server and lists its archives: `package/<n>.xs` in numeric order, then
/// `xero.xs`.
pub fn locate_install(root: &Path) -> io::Result<Install> {
    let not_install = |what: &str| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} does not look like an S4 League (Xero) install folder: {what}.\n\
                 Pick the folder that contains xerogame.exe, xero.xs and the package folder.",
                root.display()
            ),
        )
    };
    if !root.is_dir() {
        return Err(not_install("it is not a folder"));
    }
    for file in ["xerogame.exe", "xero.xs"] {
        if !root.join(file).is_file() {
            return Err(not_install(&format!("{file} is missing")));
        }
    }
    let package = root.join("package");
    if !package.is_dir() {
        return Err(not_install("the package folder is missing"));
    }
    let mut numbered = Vec::new();
    for item in fs::read_dir(&package)? {
        let item = item?;
        let path = item.path();
        let is_xs = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xs"));
        let number = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok());
        if let (true, Some(n), true) = (is_xs, number, item.file_type()?.is_file()) {
            numbered.push((n, path));
        }
    }
    if numbered.is_empty() {
        return Err(not_install("package contains no <number>.xs archives"));
    }
    numbered.sort_by_key(|(n, _)| *n);
    let mut archives: Vec<ArchiveFile> = numbered
        .into_iter()
        .map(|(_, path)| ArchiveFile { path, kind: ArchiveKind::Package })
        .collect();
    archives.push(ArchiveFile { path: root.join("xero.xs"), kind: ArchiveKind::Launcher });
    Ok(Install { root: root.to_path_buf(), archives })
}

/// Turns a stored archive path into a safe relative output path: `\` becomes
/// `/`; absolute paths, drive letters, `.`/`..` and empty components and
/// characters Windows cannot store are rejected.
pub fn normalise_path(stored: &str) -> Result<String, String> {
    let unified = stored.replace('\\', "/");
    if unified.is_empty() {
        return Err("empty path".into());
    }
    if unified.starts_with('/') {
        return Err(format!("absolute path {stored:?}"));
    }
    for part in unified.split('/') {
        match part {
            "" => return Err(format!("empty path component in {stored:?}")),
            "." | ".." => return Err(format!("relative component {part:?} in {stored:?}")),
            _ => {}
        }
        if part
            .chars()
            .any(|c| c.is_ascii_control() || matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
        {
            return Err(format!("unsupported character in {stored:?}"));
        }
        if part.ends_with(' ') || part.ends_with('.') {
            return Err(format!("component ending in a space or dot in {stored:?}"));
        }
    }
    Ok(unified)
}

/// Normalises `--only` prefixes the same way as archive paths (lowercase,
/// forward slashes, no leading slash).
pub fn normalise_prefix(prefix: &str) -> String {
    prefix.trim().replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

fn matches_only(path: &str, only: &[String]) -> bool {
    only.is_empty() || {
        let lower = path.to_ascii_lowercase();
        only.iter().any(|p| lower.starts_with(p.as_str()))
    }
}

/// One resource to write.
#[derive(Clone, Debug)]
pub struct Job {
    pub archive: usize,
    pub entry: Entry,
    /// Normalised relative output path.
    pub rel: String,
}

/// The list of resources to write, after de-duplication and filtering.
#[derive(Debug, Default)]
pub struct Plan {
    pub jobs: Vec<Job>,
    /// Entries in all archives before filtering.
    pub total_entries: usize,
    /// Entries replaced by a later archive with the same path (case-insensitive).
    pub overridden: Vec<String>,
    /// Entries whose stored path is unsafe to write: (stored path, reason).
    pub rejected: Vec<(String, String)>,
}

/// Reads the index of every archive and builds the job list. When several
/// archives hold the same path (compared case-insensitively, as on Windows),
/// the one read last wins.
pub fn plan(install: &Install, only: &[String]) -> io::Result<Plan> {
    let mut plan = Plan::default();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    for (index, archive_file) in install.archives.iter().enumerate() {
        let name = archive_file.path.display().to_string();
        let file = File::open(&archive_file.path)
            .map_err(|e| io::Error::new(e.kind(), format!("{name}: {e}")))?;
        let len = file.metadata()?.len();
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let entries = archive::parse_index(&mut reader, len)
            .map_err(|e| io::Error::new(e.kind(), format!("{name}: {e}")))?;
        plan.total_entries += entries.len();
        for entry in entries {
            let rel = match normalise_path(&entry.path) {
                Ok(rel) => rel,
                Err(reason) => {
                    if matches_only(&normalise_prefix(&entry.path), only) {
                        plan.rejected.push((entry.path, format!("{name}: {reason}")));
                    }
                    continue;
                }
            };
            if !matches_only(&rel, only) {
                continue;
            }
            let job = Job { archive: index, entry, rel };
            match by_path.get(&job.rel.to_ascii_lowercase()) {
                Some(&slot) => {
                    plan.overridden.push(plan.jobs[slot].rel.clone());
                    plan.jobs[slot] = job;
                }
                None => {
                    by_path.insert(job.rel.to_ascii_lowercase(), plan.jobs.len());
                    plan.jobs.push(job);
                }
            }
        }
    }
    // Read archives front to back for friendlier disk access.
    plan.jobs
        .sort_by_key(|j| (j.archive, j.entry.payload_offset));
    Ok(plan)
}

/// Rejects an output folder that is the install folder or inside it.
pub fn check_output_location(install_root: &Path, out: &Path) -> io::Result<()> {
    let install = fs::canonicalize(install_root)?;
    let out_abs = std::path::absolute(out)?;
    // Canonicalise the deepest existing ancestor, then re-append the rest.
    let mut existing = out_abs.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => break,
        }
    }
    let mut resolved = fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    for name in rest.iter().rev() {
        resolved.push(name);
    }
    if resolved
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "output path contains '..'"));
    }
    if resolved.starts_with(&install) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the output folder {} is inside the install folder; pick a folder elsewhere \
                 (the install folder is only read, never written)",
                out.display()
            ),
        ));
    }
    Ok(())
}

/// Counters and failures of a run.
#[derive(Debug, Default)]
pub struct Report {
    pub written: u64,
    pub skipped: u64,
    pub written_bytes: u64,
    pub decoded_written: u64,
    pub decoded_skipped: u64,
    pub failures: Vec<(String, String)>,
    pub elapsed: Duration,
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0;
    while done < buf.len() {
        let n = file.seek_read(&mut buf[done..], offset)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "archive is truncated"));
        }
        done += n;
        offset += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)
}

fn has_len(path: &Path, len: u64) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == len)
}

/// Writes through a `.partial` file and renames it, so an interrupted run
/// never leaves a full-size but incomplete file behind.
fn write_file(target: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut partial = target.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    fs::write(&partial, data)?;
    fs::rename(&partial, target)
}

fn is_scene(rel: &str) -> bool {
    rel.len() >= 4 && rel[rel.len() - 4..].eq_ignore_ascii_case(".scn")
}

#[derive(Default)]
struct Outcome {
    written: bool,
    bytes: u64,
    decoded_written: bool,
    decoded_skipped: bool,
}

fn run_job(job: &Job, install: &Install, files: &[File], out: &Path) -> io::Result<Outcome> {
    let mut outcome = Outcome::default();
    let target = out.join(RECOVERED_DIR).join(&job.rel);
    let scene_target = out.join(DECODED_DIR).join(&job.rel);

    let data = if has_len(&target, job.entry.original_len) {
        if !is_scene(&job.rel) || has_len(&scene_target, job.entry.original_len) {
            if is_scene(&job.rel) {
                outcome.decoded_skipped = true;
            }
            return Ok(outcome);
        }
        // The resource is already there but its decoded scene may be missing.
        let mut head = [0u8; 8];
        let ok = File::open(&target).and_then(|mut f| f.read_exact(&mut head)).is_ok();
        if !ok || !scene::has_signature(&head) {
            return Ok(outcome);
        }
        fs::read(&target)?
    } else {
        let mut stored = vec![0u8; job.entry.stored_len as usize];
        read_at(&files[job.archive], &mut stored, job.entry.payload_offset)?;
        let kind = install.archives[job.archive].kind;
        let data = archive::decode_payload(&job.entry, kind, &stored)?;
        write_file(&target, &data)?;
        outcome.written = true;
        outcome.bytes = data.len() as u64;
        data
    };

    if is_scene(&job.rel) && scene::has_signature(&data) {
        if has_len(&scene_target, data.len() as u64) {
            outcome.decoded_skipped = true;
        } else {
            let decoded = scene::decode(&data).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CC01 scene is shorter than its 12-byte header")
            })?;
            write_file(&scene_target, &decoded)?;
            outcome.decoded_written = true;
        }
    }
    Ok(outcome)
}

/// Extracts every job of `plan` into `out`, in parallel. `progress` is
/// called about once per second with (files done, files total, bytes written).
pub fn run(
    install: &Install,
    plan: &Plan,
    out: &Path,
    progress: &(dyn Fn(u64, u64, u64) + Sync),
) -> io::Result<Report> {
    let started = Instant::now();
    let files = install
        .archives
        .iter()
        .map(|a| File::open(&a.path))
        .collect::<io::Result<Vec<_>>>()?;
    fs::create_dir_all(out.join(RECOVERED_DIR))?;
    fs::create_dir_all(out.join(DECODED_DIR))?;

    let done = AtomicU64::new(0);
    let written = AtomicU64::new(0);
    let skipped = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let decoded_written = AtomicU64::new(0);
    let decoded_skipped = AtomicU64::new(0);
    let failures = Mutex::new(Vec::new());
    let last_print = Mutex::new(Instant::now());
    let total = plan.jobs.len() as u64;

    plan.jobs.par_iter().for_each(|job| {
        match run_job(job, install, &files, out) {
            Ok(o) => {
                if o.written {
                    written.fetch_add(1, Ordering::Relaxed);
                    bytes.fetch_add(o.bytes, Ordering::Relaxed);
                } else {
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
                if o.decoded_written {
                    decoded_written.fetch_add(1, Ordering::Relaxed);
                }
                if o.decoded_skipped {
                    decoded_skipped.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                let archive = install.archives[job.archive]
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                failures
                    .lock()
                    .expect("failure list")
                    .push((job.entry.path.clone(), format!("{archive}: {e}")));
            }
        }
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut last) = last_print.try_lock()
            && last.elapsed() >= Duration::from_secs(1)
        {
            *last = Instant::now();
            progress(n, total, bytes.load(Ordering::Relaxed));
        }
    });
    progress(total, total, bytes.load(Ordering::Relaxed));

    let mut failures = failures.into_inner().expect("failure list");
    failures.sort();
    Ok(Report {
        written: written.into_inner(),
        skipped: skipped.into_inner(),
        written_bytes: bytes.into_inner(),
        decoded_written: decoded_written.into_inner(),
        decoded_skipped: decoded_skipped.into_inner(),
        failures,
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_normalisation() {
        assert_eq!(normalise_path("xml/item.xml").unwrap(), "xml/item.xml");
        assert_eq!(normalise_path("model\\a\\b.scn").unwrap(), "model/a/b.scn");
        assert_eq!(normalise_path("pass event_play_popup.tga").unwrap(), "pass event_play_popup.tga");
        for bad in ["", "/etc/x", "\\x", "c:/x", "a/../b", "..", "a/./b", "a//b", "a/", "a\0b", "a./b"] {
            assert!(normalise_path(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn only_prefixes() {
        let only = vec![normalise_prefix("Model\\Character"), normalise_prefix("/xml")];
        assert!(matches_only("model/character/a.scn", &only));
        assert!(matches_only("xml/a.xml", &only));
        assert!(!matches_only("effects/a.dds", &only));
        assert!(matches_only("anything", &[]));
    }

    #[test]
    fn scene_extension() {
        assert!(is_scene("a/b.scn"));
        assert!(is_scene("a/B.SCN"));
        assert!(!is_scene("a/b.scnx"));
        assert!(!is_scene("scn"));
    }
}