//! Repository tasks, run as `cargo xtask <task>`.
//!
//! `cargo xtask check` runs the compiling checks a change needs before it
//! merges; the git hooks run only the checks that compile nothing. Clippy,
//! the tests and the doctests run on the host, and off Windows clippy also
//! runs for Windows through `cargo xwin`, beside the host checks. `cargo
//! xtask check --all` adds formatting, rustdoc and cargo-deny.

use std::io::Write as _;
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Instant;

const USAGE: &str = "usage: cargo xtask check [--all] [--base <git ref>]";
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
