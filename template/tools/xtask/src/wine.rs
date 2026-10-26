//! The tests of the crates that build only for Windows, run off Windows under
//! Wine in a Docker container.
//!
//! `cargo xwin test` builds them, and cargo hands every test executable,
//! doctests included, to `cargo xtask wine-run`, which runs it in the image
//! built from `tools/wine/Dockerfile`. On an amd64 Linux host with Wine
//! installed, as on CI, the tests run under that Wine directly. Under Apple
//! silicon the container runs
//! through Rosetta, which cannot emulate exception unwinding, stack walks,
//! thread suspension or code patching; those tests are skipped there and run
//! on Windows and on amd64 hosts.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::{Step, WINDOWS_TARGET, say};

const IMAGE: &str = "enshwick-wine-test:trixie";
const RUNNER_ENV: &str = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER";
/// Test name filters Rosetta aborts the process on.
const ROSETTA_SKIPS: &[&str] = &["crash::", "detour::", "patch", "thread"];
/// Paths to game executables that some tests read when they are set.
const GAME_EXES: &[&str] = &["ENSH_CLIENT_EXE", "ENSH_SERVER_EXE"];
const DOCKER_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a run waits for another worktree's run to finish.
const LOCK_TIMEOUT: Duration = Duration::from_mins(30);
/// How long one test executable may run before its container is killed.
const RUN_TIMEOUT: Duration = Duration::from_mins(10);

/// Runs the tests of `packages` under Wine. Without a running Docker the step
/// is skipped with a message, and fails only when it was asked for by name
/// (`required`).
pub(crate) fn run(packages: &[&str], required: bool) -> bool {
    if packages.is_empty() {
        return true;
    }
    if cfg!(windows) {
        say("on Windows the Windows-only crates run with the host tests");
        return true;
    }
    if native_wine() {
        return tests(packages).run(false);
    }
    if !docker_ready() {
        say(&format!(
            "docker is not running: the tests of {} under Wine are {}",
            packages.join(", "),
            if required { "required" } else { "skipped" }
        ));
        return !required;
    }
    // Several amd64 containers under Rosetta at once have hung the Docker
    // daemon: one run at a time on the machine.
    let Some(_lock) = machine_lock() else {
        return false;
    };
    if !build_image() {
        return false;
    }
    let Ok(xtask) = std::env::current_exe() else {
        say("cannot find the xtask executable to run tests with");
        return false;
    };
    let mut step = tests(packages);
    if cfg!(target_arch = "aarch64") {
        for skip in ROSETTA_SKIPS {
            step = step.then(&["--skip", skip]);
        }
    }
    step.env(RUNNER_ENV, format!("{} wine-run", xtask.display()))
        .run(false)
}

/// `cargo xwin test` of `packages`, one test at a time: the fake host keeps
/// global state. cargo-xwin runs the executables with `wine` unless a runner
/// is set.
fn tests(packages: &[&str]) -> Step {
    let mut step = Step::new(&["xwin", "test", "--target", WINDOWS_TARGET]);
    for package in packages {
        step = step.then(&["-p", package]);
    }
    step.then(&["--", "--test-threads", "1"])
}

/// Whether Wine runs natively here: an amd64 Linux host with `wine`.
fn native_wine() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
        && Command::new("wine")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

/// Runs one Windows executable under Wine with `args`; cargo calls this as
/// the target runner.
pub(crate) fn run_one(args: &[String]) -> Result<bool, String> {
    let (exe, args) = args
        .split_first()
        .ok_or("usage: cargo xtask wine-run <exe> [args]")?;
    let exe = Path::new(exe);
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else {
        return Err(format!("{} is not a file path", exe.display()));
    };
    let root = workspace_root();
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let container = format!("enshwick-wine-{}", std::process::id());
    let mut command = Command::new("docker");
    command
        .args([
            "run",
            "--rm",
            "--platform",
            "linux/amd64",
            "--name",
            &container,
        ])
        .args(["-e", "WINEDEBUG=-all"])
        .arg("-v")
        .arg(format!("{}:/w:ro", root.display()))
        .arg("-v")
        .arg(format!("{}:/d:ro", dir.display()));
    for var in GAME_EXES {
        let Some(path) = std::env::var_os(var).map(PathBuf::from) else {
            continue;
        };
        if let (Some(parent), Some(file)) = (path.parent(), path.file_name()) {
            command
                .arg("-v")
                .arg(format!("{}:/ensh/{var}:ro", parent.display()))
                .arg("-e")
                .arg(format!("{var}=Z:\\ensh\\{var}\\{}", file.to_string_lossy()));
        }
    }
    // OrbStack does not bind-mount at a macOS path inside a container, and
    // tests read sources at the absolute `CARGO_MANIFEST_DIR`: the workspace
    // mounts at /w and a link gives it its host path back.
    command
        .args([
            IMAGE,
            "bash",
            "-c",
            r#"mkdir -p "$(dirname "$1")" && ln -s /w "$1" && { cd "$2" 2>/dev/null || cd /; } && shift 2 && exec wine "$@""#,
            "bash",
        ])
        .arg(&root)
        .arg(&cwd)
        .arg(format!("/d/{}", name.to_string_lossy()))
        .args(args);
    let child = command
        .spawn()
        .map_err(|error| format!("running docker: {error}"))?;
    if let Some(ok) = wait(child, RUN_TIMEOUT) {
        return Ok(ok);
    }
    let _killed = Command::new("docker")
        .args(["kill", &container])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Err(format!(
        "{} ran over {} minutes; its container was killed",
        name.to_string_lossy(),
        RUN_TIMEOUT.as_secs() / 60
    ))
}

/// Holds the machine-wide lock of Wine runs, waiting for another run to
/// finish; the lock goes with the file when the process ends.
fn machine_lock() -> Option<File> {
    let path = std::env::temp_dir().join("enshwick-wine-test.lock");
    let Ok(file) = File::create(&path) else {
        say(&format!("cannot create {}", path.display()));
        return None;
    };
    let started = Instant::now();
    let mut told = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(TryLockError::WouldBlock) if started.elapsed() < LOCK_TIMEOUT => {
                if !told {
                    say("waiting for another Wine test run on this machine to finish");
                    told = true;
                }
                thread::sleep(Duration::from_secs(1));
            }
            Err(TryLockError::WouldBlock) => {
                say(&format!(
                    "another Wine test run held {} for {} minutes; giving up",
                    path.display(),
                    LOCK_TIMEOUT.as_secs() / 60
                ));
                return None;
            }
            Err(TryLockError::Error(error)) => {
                say(&format!("locking {}: {error}", path.display()));
                return None;
            }
        }
    }
}

/// Waits for `child` up to `timeout`: whether it succeeded, or `None` when
/// it is still running, killed by then.
fn wait(mut child: Child, timeout: Duration) -> Option<bool> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status.success()),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _killed = child.kill();
                let _reaped = child.wait();
                return None;
            }
            Err(_) => return Some(false),
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Whether the Docker daemon answers within a few seconds; a hung daemon
/// counts as not running.
fn docker_ready() -> bool {
    Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .and_then(|child| wait(child, DOCKER_TIMEOUT))
        == Some(true)
}

/// Builds the image; Docker's layer cache makes this a second when the
/// Dockerfile is unchanged.
fn build_image() -> bool {
    let context = workspace_root().join("tools/wine");
    let started = Instant::now();
    let ok = Command::new("docker")
        .args(["build", "--quiet", "--platform", "linux/amd64", "-t", IMAGE])
        .arg(&context)
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let seconds = started.elapsed().as_secs_f32();
    say(&format!(
        "{} in {seconds:.1} s: docker build -t {IMAGE} tools/wine",
        if ok { "ok" } else { "FAILED" }
    ));
    ok
}
