# s4-xero-extract

An unofficial fan tool that extracts the resource archives of an **S4 League**
client you have installed yourself, so that an S4 League engine
reimplementation can load the files locally. It reads the S4 League client
distributed by the **Xero** private server; that client is the one the tool is
made and tested against.

This repository contains only the tool's own source code and documentation. It
ships **no game files**: no archives, models, textures, sounds, scripts or
extracted output of any kind. You need your own installed client.

The tool is not affiliated with, sponsored by or endorsed by the S4 League
rights holders or by Xero. See [LEGAL.md](LEGAL.md).

## What it does

Given the Xero install folder (the one with `xerogame.exe`, `xero.xs` and
`package/0.xs`, `package/1.xs`, ...), it writes:

```
<output>/
  recovered/<path>   every resource in the archives, decoded, at the path
                     stored in the archive (e.g. recovered/xml/..., recovered/model/...)
  decoded/<path>     every .scn scene in the encrypted "CC01" variant, decrypted
                     (other files are not copied here)
```

Point the engine at `<output>`, the folder that contains both `recovered` and
`decoded`.

Every resource is checked against the length and CRC32 stored in its archive;
a mismatch is reported as a failure and nothing broken is written. The install
folder is only read, never modified, and the output folder may not be inside
it. Archive paths that are absolute or contain `..` are rejected.

A full extraction writes about 57,300 files (about 55,700 resources plus 1,700
decoded scenes) totalling about 22.8 GB (21.2 GiB); plan for roughly 25 GB of
free space. On an SSD it takes a few minutes.

## Usage

**Double-click** `s4-xero-extract.exe` (or run it without arguments). It asks
for the install folder, then for the output folder, shows progress and waits
for Enter at the end.

**Command line:**

```
s4-xero-extract <install folder> <output folder> [--only <prefix>[,<prefix>...]]
```

- `--only model/character,xml` limits the run to resources whose path starts
  with one of the prefixes.
- Files that already exist in the output with the right size are skipped, so an
  interrupted run can simply be started again with the same arguments.
- The exit code is 0 when everything was extracted, 1 if any resource failed and
  2 for usage errors.

Example:

```
s4-xero-extract "D:\Games\S4League" "D:\S4Data"
```

## Building

Install Rust (https://rustup.rs), then:

```
cargo build --release
```

The executable is `target/release/s4-xero-extract(.exe)`. On Windows,
`build-release.ps1` builds the same with local profile paths removed from the
binary and packs it with the documentation and licence texts into
`dist/s4-xero-extract-<version>-windows-x64.zip`.

Run the tests with `cargo test`; they use small synthetic archives built in the
tests, not game files.

## Rules for use

- Use this tool only on an S4 League client that you have installed yourself.
- The extracted files are for your **personal, local use** with a compatible
  engine. Do not share, upload or redistribute them, or post them in issues,
  pull requests or anywhere else.
- When reporting a bug, describe it in your own words and do not attach game
  files or extracted output.

## Licence

GPL-3.0-only, see [LICENSE](LICENSE). Dependencies keep their own licences; see
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and `licenses/`. No licence
to any game content, code or trademark is granted.
