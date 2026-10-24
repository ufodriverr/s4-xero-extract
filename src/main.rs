//! Command-line front end. With no arguments it asks for the folders with
//! dialogs and waits for Enter at the end (for double-click use on Windows).

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s4_xero_extract::extract::{self, DECODED_DIR, RECOVERED_DIR};

const USAGE: &str = "\
Usage:
  s4-xero-extract <install folder> <output folder> [--only <prefix>[,<prefix>...]]
  s4-xero-extract            (no arguments: pick both folders in dialogs)

  <install folder>  your S4 League client from the Xero private server
                    (the folder with xerogame.exe, xero.xs and package/)
  <output folder>   receives recovered/ (every resource) and decoded/
                    (decrypted CC01 scenes); must not be inside the install
  --only            only extract resources whose path starts with one of the
                    comma-separated prefixes, e.g. --only model/character,xml

Files that already exist in the output with the right size are skipped, so an
interrupted run can simply be started again.";

struct Args {
    install: PathBuf,
    output: PathBuf,
    only: Vec<String>,
}

fn parse_args(raw: Vec<String>) -> Result<Option<Args>, String> {
    let mut positional = Vec::new();
    let mut only = Vec::new();
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(None);
        } else if arg == "-V" || arg == "--version" {
            println!("s4-xero-extract {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        } else if arg == "--only" {
            let value = it.next().ok_or("--only needs a value")?;
            only.extend(split_prefixes(&value));
        } else if let Some(value) = arg.strip_prefix("--only=") {
            only.extend(split_prefixes(value));
        } else if arg.starts_with("--") {
            return Err(format!("unknown option {arg}"));
        } else {
            positional.push(PathBuf::from(arg));
        }
    }
    if positional.len() != 2 {
        return Err("expected an install folder and an output folder".into());
    }
    let output = positional.pop().expect("two positionals");
    let install = positional.pop().expect("two positionals");
    Ok(Some(Args { install, output, only }))
}

fn split_prefixes(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(extract::normalise_prefix)
        .filter(|p| !p.is_empty())
        .collect()
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn extract_all(install_root: &Path, output: &Path, only: &[String]) -> Result<bool, String> {
    let install = extract::locate_install(install_root).map_err(|e| e.to_string())?;
    extract::check_output_location(install_root, output).map_err(|e| e.to_string())?;

    println!("Install: {}", install.root.display());
    println!("Output:  {}", output.display());
    println!("Reading {} archive indexes...", install.archives.len());
    let plan = extract::plan(&install, only).map_err(|e| e.to_string())?;
    let total_bytes: u64 = plan.jobs.iter().map(|j| j.entry.original_len).sum();
    println!(
        "{} resources in the archives, {} selected ({}).",
        plan.total_entries,
        plan.jobs.len(),
        human_bytes(total_bytes)
    );
    if !only.is_empty() {
        println!("Limited to: {}", only.join(", "));
    }
    if !plan.overridden.is_empty() {
        println!(
            "{} paths appear in more than one archive; the later archive's copy is used.",
            plan.overridden.len()
        );
    }

    let progress = |done: u64, total: u64, bytes: u64| {
        println!("  {done:>6} / {total} files   {} written", human_bytes(bytes));
    };
    let report = extract::run(&install, &plan, output, &progress).map_err(|e| e.to_string())?;

    println!();
    println!(
        "Done in {:.1} s: {} written ({}), {} already present.",
        report.elapsed.as_secs_f64(),
        report.written,
        human_bytes(report.written_bytes),
        report.skipped
    );
    println!(
        "CC01 scenes: {} decoded, {} already present.",
        report.decoded_written, report.decoded_skipped
    );
    println!(
        "Output: {} and {}",
        output.join(RECOVERED_DIR).display(),
        output.join(DECODED_DIR).display()
    );

    let mut problems: Vec<(String, String)> = plan.rejected.clone();
    problems.extend(report.failures);
    if problems.is_empty() {
        return Ok(true);
    }
    eprintln!();
    eprintln!("{} resources FAILED:", problems.len());
    for (path, error) in problems.iter().take(50) {
        eprintln!("  {path}: {error}");
    }
    if problems.len() > 50 {
        eprintln!("  ... and {} more", problems.len() - 50);
    }
    Ok(false)
}

fn wait_for_enter() {
    print!("\nPress Enter to close this window.");
    let _ = io::stdout().flush();
    let _ = io::stdin().lock().read_line(&mut String::new());
}

fn interactive() -> ExitCode {
    println!("s4-xero-extract {}", env!("CARGO_PKG_VERSION"));
    println!("Unofficial fan tool. Extracts the resources of your own installed S4 League");
    println!("client (Xero private server) for local use. Nothing is uploaded or shared.\n");

    let install = loop {
        println!("Select your S4 League (Xero) install folder (the one with xerogame.exe)...");
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Select the S4 League (Xero) install folder")
            .pick_folder()
        else {
            println!("Cancelled.");
            wait_for_enter();
            return ExitCode::from(2);
        };
        match extract::locate_install(&folder) {
            Ok(_) => break folder,
            Err(e) => println!("{e}\n"),
        }
    };
    let output = loop {
        println!("Select the output folder (recovered/ and decoded/ go inside it)...");
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Select the output folder")
            .pick_folder()
        else {
            println!("Cancelled.");
            wait_for_enter();
            return ExitCode::from(2);
        };
        match extract::check_output_location(&install, &folder) {
            Ok(()) => break folder,
            Err(e) => println!("{e}\n"),
        }
    };
    println!();
    let code = finish(extract_all(&install, &output, &[]));
    wait_for_enter();
    code
}

fn finish(result: Result<bool, String>) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.is_empty() {
        return interactive();
    }
    match parse_args(raw) {
        Ok(Some(args)) => finish(extract_all(&args.install, &args.output, &args.only)),
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
