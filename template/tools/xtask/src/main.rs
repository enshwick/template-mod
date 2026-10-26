//! Repository tasks, run as `cargo xtask <task>`.
//!
//! `cargo xtask check` runs the compiling checks a change needs before it
//! merges; the git hooks run only the checks that compile nothing. Clippy,
//! the tests and the doctests run on the host, where the packages that build
//! only for Windows (`[package.metadata.enshwick] windows-only = true`) stay
//! out; off Windows clippy also runs for Windows through `cargo xwin`,
//! beside the host checks. `--windows` runs the Windows-only packages' tests
//! under Wine, and `--all` adds formatting, rustdoc, cargo-deny and those
//! tests.
//!
//! `cargo xtask dist` builds the release DLLs and writes what a player
//! downloads: `<package>.zip` with `mods/<package>.dll` and the license, and
//! the PDB beside it, for every mod in the workspace.

mod wine;

use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Instant;

const USAGE: &str = "usage: cargo xtask check [--all] [--windows] [--base <git ref>] | cargo xtask dist [--out <dir>]";
const WINDOWS_TARGET: &str = "x86_64-pc-windows-msvc";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            say(&error);
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<bool, String> {
    let (task, rest) = args.split_first().ok_or(USAGE)?;
    match task.as_str() {
        "dist" => return dist(rest),
        "wine-run" => return wine::run_one(rest),
        "check" => {}
        _ => return Err(USAGE.to_owned()),
    }
    let mut all = false;
    let mut windows = false;
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--all" => all = true,
            "--windows" => windows = true,
            // A mod repository is small: every change checks all of it.
            "--base" => {
                rest.next().ok_or(USAGE)?;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    let packages = packages()?;
    let mut ok = check(&packages, all);
    if ok && (all || windows) {
        let windows_only: Vec<&str> = packages
            .iter()
            .filter(|p| p.windows_only)
            .map(|p| p.name.as_str())
            .collect();
        ok = windows_only.is_empty() || wine::run(&windows_only, windows);
    }
    Ok(ok)
}

/// A workspace package.
#[derive(Debug)]
struct Package {
    name: String,
    windows_only: bool,
    /// Whether it builds a DLL: a mod.
    cdylib: bool,
}

/// The workspace packages, from `cargo metadata`.
fn packages() -> Result<Vec<Package>, String> {
    let output = Command::new(cargo())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|error| format!("running cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("reading cargo metadata: {error}"))?;
    Ok(metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|package| Package {
            name: package["name"].as_str().unwrap_or_default().to_owned(),
            windows_only: package["metadata"]["enshwick"]["windows-only"].as_bool() == Some(true),
            cdylib: package["targets"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|target| {
                    target["crate_types"]
                        .as_array()
                        .is_some_and(|types| types.iter().any(|t| t == "cdylib"))
                }),
        })
        .collect())
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// One cargo command line.
#[derive(Debug)]
struct Step {
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
}

impl Step {
    fn new(words: &[&str]) -> Self {
        Self {
            args: words.iter().map(|w| (*w).to_owned()).collect(),
            env: Vec::new(),
        }
    }

    fn then_owned(mut self, words: &[String]) -> Self {
        self.args.extend_from_slice(words);
        self
    }

    fn then(mut self, words: &[&str]) -> Self {
        self.args.extend(words.iter().map(|w| (*w).to_owned()));
        self
    }

    fn env(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.env.push((name, value.into()));
        self
    }

    fn line(&self) -> String {
        format!("cargo {}", self.args.join(" "))
    }

    /// Runs the step with its output shown, or captured when `quiet` (for a
    /// step running beside others) and shown only when it fails.
    fn run(&self, quiet: bool) -> bool {
        let started = Instant::now();
        let mut command = Command::new(cargo());
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(name, value)| (name, value)));
        let (ok, output) = if quiet {
            match command.output() {
                Ok(output) => (
                    output.status.success(),
                    [output.stdout, output.stderr].concat(),
                ),
                Err(error) => (false, error.to_string().into_bytes()),
            }
        } else {
            say(&format!("> {}", self.line()));
            match command.status() {
                Ok(status) => (status.success(), Vec::new()),
                Err(error) => (false, error.to_string().into_bytes()),
            }
        };
        let seconds = started.elapsed().as_secs_f32();
        if ok {
            say(&format!("ok in {seconds:.1} s: {}", self.line()));
        } else {
            let _shown = std::io::stderr().write_all(&output);
            say(&format!("FAILED in {seconds:.1} s: {}", self.line()));
        }
        ok
    }
}

fn say(text: &str) {
    let _shown = writeln!(std::io::stderr(), "xtask: {text}");
}

/// Runs the host steps one after another (they share the build lock) and
/// the Windows lint beside them; the first failing host step ends the host
/// checks.
fn check(packages: &[Package], all: bool) -> bool {
    let mut workspace = vec!["--workspace".to_owned()];
    if !cfg!(windows) {
        for package in packages.iter().filter(|p| p.windows_only) {
            workspace.extend(["--exclude".to_owned(), package.name.clone()]);
        }
    }
    let host = |words: &[&str]| Step::new(words).then_owned(&workspace);
    let mut steps = Vec::new();
    if all {
        steps.push(Step::new(&["fmt", "--all", "--check"]));
    }
    steps.push(host(&["clippy", "--all-targets"]).then(&["--", "-D", "warnings"]));
    steps.push(host(&["nextest", "run", "--no-tests=pass"]));
    steps.push(host(&["test", "--doc"]));
    if all {
        steps.push(host(&["doc", "--no-deps"]).env("RUSTDOCFLAGS", "-D warnings"));
        steps.push(Step::new(&["deny", "check"]));
    }
    let windows = (!cfg!(windows)).then(|| {
        Step::new(&[
            "xwin",
            "clippy",
            "--target",
            WINDOWS_TARGET,
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
    });
    if let Some(step) = &windows {
        say(&format!("> {} (beside the host checks)", step.line()));
    }
    thread::scope(|scope| {
        let windows = windows.as_ref().map(|step| scope.spawn(|| step.run(true)));
        let host_passed = steps.iter().all(|step| step.run(false));
        let windows_passed = windows.is_none_or(|handle| handle.join().unwrap_or(false));
        host_passed && windows_passed
    })
}

/// Builds the release DLLs and writes each mod's archive and PDB into
/// `--out` (`target/dist` by default).
fn dist(args: &[String]) -> Result<bool, String> {
    let out = match args {
        [] => PathBuf::from("target/dist"),
        [flag, dir] if flag == "--out" => PathBuf::from(dir),
        _ => return Err(USAGE.to_owned()),
    };
    let command: &[&str] = if cfg!(windows) {
        &[
            "build",
            "--workspace",
            "--release",
            "--locked",
            "--target",
            WINDOWS_TARGET,
        ]
    } else {
        &[
            "xwin",
            "build",
            "--workspace",
            "--release",
            "--locked",
            "--target",
            WINDOWS_TARGET,
        ]
    };
    if !Step::new(command).run(false) {
        return Ok(false);
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let license = read(Path::new("LICENSE"))?;
    for package in packages()?.iter().filter(|p| p.cdylib) {
        let name = &package.name;
        let built = Path::new("target")
            .join(WINDOWS_TARGET)
            .join("release")
            .join(name.replace('-', "_"));
        let entries = [
            (
                format!("mods/{name}.dll"),
                read(&built.with_extension("dll"))?,
            ),
            ("LICENSE".to_owned(), license.clone()),
        ];
        let archive = out.join(format!("{name}.zip"));
        std::fs::write(&archive, zip(&entries)?)
            .map_err(|e| format!("{}: {e}", archive.display()))?;
        let pdb = out.join(format!("{name}.pdb"));
        std::fs::copy(built.with_extension("pdb"), &pdb)
            .map_err(|e| format!("{}: {e}", pdb.display()))?;
        say(&format!(
            "wrote {} and {}",
            archive.display(),
            pdb.display()
        ));
    }
    Ok(true)
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The archive of `entries` in their order and with the fixed time of the
/// format's epoch, so the same files give the same bytes.
fn zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    for (path, content) in entries {
        archive
            .start_file(path.as_str(), options)
            .and_then(|()| archive.write_all(content).map_err(Into::into))
            .map_err(|e| format!("{path}: {e}"))?;
    }
    archive
        .finish()
        .map(Cursor::into_inner)
        .map_err(|e| e.to_string())
}
