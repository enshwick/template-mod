//! Repository tasks, run as `cargo xtask <task>`.
//!
//! `cargo xtask check` runs the compiling checks a change needs before it
//! merges; the git hooks run only the checks that compile nothing. Clippy,
//! the tests and the doctests run on the host, and off Windows clippy also
//! runs for Windows through `cargo xwin`, beside the host checks. `cargo
//! xtask check --all` adds formatting, rustdoc and cargo-deny.
//!
//! `cargo xtask dist` builds the release DLL and writes what a player
//! downloads: `<package>.zip` with `mods/<package>.dll` and the license, and
//! the PDB beside it.

use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Instant;

const USAGE: &str =
    "usage: cargo xtask check [--all] [--base <git ref>] | cargo xtask dist [--out <dir>]";
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
    if task == "dist" {
        return dist(rest);
    }
    if task != "check" {
        return Err(USAGE.to_owned());
    }
    let mut all = false;
    let mut rest = rest.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--all" => all = true,
            // One crate: every change checks all of it.
            "--base" => {
                rest.next().ok_or(USAGE)?;
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    Ok(check(all))
}

/// One cargo command line with its environment.
#[derive(Debug)]
struct Step {
    args: Vec<&'static str>,
    env: Vec<(&'static str, &'static str)>,
}

impl Step {
    fn new(args: &[&'static str]) -> Self {
        Self {
            args: args.to_vec(),
            env: Vec::new(),
        }
    }

    fn env(mut self, name: &'static str, value: &'static str) -> Self {
        self.env.push((name, value));
        self
    }

    fn line(&self) -> String {
        format!("cargo {}", self.args.join(" "))
    }

    /// Runs the step with its output shown, or captured when `quiet` (for a
    /// step running beside others) and shown only when it fails.
    fn run(&self, quiet: bool) -> bool {
        let started = Instant::now();
        let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        command.args(&self.args).envs(self.env.iter().copied());
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
fn check(all: bool) -> bool {
    let mut host = Vec::new();
    if all {
        host.push(Step::new(&["fmt", "--all", "--check"]));
    }
    host.push(Step::new(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]));
    host.push(Step::new(&[
        "nextest",
        "run",
        "--workspace",
        "--no-tests=pass",
    ]));
    host.push(Step::new(&["test", "--doc", "--workspace"]));
    if all {
        host.push(
            Step::new(&["doc", "--no-deps", "--workspace"]).env("RUSTDOCFLAGS", "-D warnings"),
        );
        host.push(Step::new(&["deny", "check"]));
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
        let host_passed = host.iter().all(|step| step.run(false));
        let windows_passed = windows.is_none_or(|handle| handle.join().unwrap_or(false));
        host_passed && windows_passed
    })
}

/// Builds the release DLL and writes the player's archive and the PDB into
/// `--out` (`target/dist` by default).
fn dist(args: &[String]) -> Result<bool, String> {
    let out = match args {
        [] => PathBuf::from("target/dist"),
        [flag, dir] if flag == "--out" => PathBuf::from(dir),
        _ => return Err(USAGE.to_owned()),
    };
    let command: &[&'static str] = if cfg!(windows) {
        &["build", "--release", "--locked", "--target", WINDOWS_TARGET]
    } else {
        &[
            "xwin",
            "build",
            "--release",
            "--locked",
            "--target",
            WINDOWS_TARGET,
        ]
    };
    if !Step::new(command).run(false) {
        return Ok(false);
    }
    let package = package_name()?;
    let built = Path::new("target")
        .join(WINDOWS_TARGET)
        .join("release")
        .join(package.replace('-', "_"));
    let entries = [
        (
            format!("mods/{package}.dll"),
            read(&built.with_extension("dll"))?,
        ),
        ("LICENSE".to_owned(), read(Path::new("LICENSE"))?),
    ];
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let archive = out.join(format!("{package}.zip"));
    std::fs::write(&archive, zip(&entries)?).map_err(|e| format!("{}: {e}", archive.display()))?;
    let pdb = out.join(format!("{package}.pdb"));
    std::fs::copy(built.with_extension("pdb"), &pdb)
        .map_err(|e| format!("{}: {e}", pdb.display()))?;
    say(&format!(
        "wrote {} and {}",
        archive.display(),
        pdb.display()
    ));
    Ok(true)
}

/// The package name, the first `name` in the root manifest.
fn package_name() -> Result<String, String> {
    let manifest = String::from_utf8(read(Path::new("Cargo.toml"))?).map_err(|e| e.to_string())?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("name = "))
        .map(|name| name.trim_matches('"').to_owned())
        .ok_or_else(|| "Cargo.toml names no package".to_owned())
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
