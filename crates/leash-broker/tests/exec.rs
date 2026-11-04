//! The executor against real processes.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{bind_copy, system_program};
use leash_broker::{Limits, PinError, PinnedBinary, run_pinned};

const TIMEOUT: Duration = Duration::from_secs(10);

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

async fn run(
    path: &str,
    hash: &str,
    argv: &[&str],
    limits: Limits,
    timeout: Duration,
    dir: &std::path::Path,
) -> leash_broker::Execution {
    let pinned = PinnedBinary::open(path, hash).expect("pin");
    run_pinned(&pinned, &strings(argv), limits, timeout, dir)
        .await
        .expect("run")
}

#[tokio::test]
async fn runs_the_pinned_program_and_captures_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "echo", "say");
    let result = run(
        &path,
        &hash,
        &["hello"],
        Limits::default(),
        TIMEOUT,
        dir.path(),
    )
    .await;
    assert!(result.succeeded());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "hello\n");
}

#[test]
fn a_different_file_at_the_same_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "echo", "say");
    std::fs::remove_file(&path).unwrap();
    std::fs::copy(system_program("true"), &path).unwrap();
    assert!(matches!(
        PinnedBinary::open(&path, &hash),
        Err(PinError::HashMismatch { .. })
    ));
}

#[tokio::test]
async fn swapping_the_file_after_the_check_does_not_change_what_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "echo", "say");
    let pinned = PinnedBinary::open(&path, &hash).expect("pin");

    // The race a path-based check loses: between "hash is right" and "start it", someone replaces the file.
    let evil = dir.path().join("evil");
    std::fs::copy(system_program("true"), &evil).unwrap();
    std::fs::rename(&evil, &path).unwrap();

    let result = run_pinned(
        &pinned,
        &strings(&["original"]),
        Limits::default(),
        TIMEOUT,
        dir.path(),
    )
    .await
    .expect("run");
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "original\n",
        "the verified file ran, not the replacement (which prints nothing)"
    );
}

#[test]
fn a_symlink_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (real, hash) = bind_copy(dir.path(), "echo", "real");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(matches!(
        PinnedBinary::open(link.to_str().unwrap(), &hash),
        Err(PinError::Open { .. })
    ));
}

#[test]
fn scripts_and_writable_files_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();

    let script = dir.path().join("script");
    std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script_hash = leash_broker::sha256_of(&script).unwrap();
    assert!(matches!(
        PinnedBinary::open(script.to_str().unwrap(), &script_hash),
        Err(PinError::NotElf(_))
    ));

    let (path, hash) = bind_copy(dir.path(), "echo", "loose");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o757)).unwrap();
    assert!(matches!(
        PinnedBinary::open(&path, &hash),
        Err(PinError::Writable(_))
    ));

    assert!(matches!(
        PinnedBinary::open(dir.path().to_str().unwrap(), &hash),
        Err(PinError::NotRegular(_))
    ));
}

#[tokio::test]
async fn the_child_gets_an_empty_environment_and_the_given_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    // SAFETY-free way to put something in this process's environment: the test passes it through a variable the
    // runtime already has.
    let (env_path, env_hash) = bind_copy(dir.path(), "env", "printenv");
    let result = run(
        &env_path,
        &env_hash,
        &[],
        Limits::default(),
        TIMEOUT,
        work.path(),
    )
    .await;
    let printed = String::from_utf8_lossy(&result.stdout).into_owned();
    let names: Vec<&str> = printed
        .lines()
        .filter_map(|l| l.split('=').next())
        .collect();
    assert!(
        names.iter().all(|n| matches!(*n, "PATH" | "LC_ALL")),
        "unexpected variables: {names:?}"
    );
    assert!(
        std::env::var_os("HOME").is_some(),
        "the test process has a HOME that must not leak"
    );
    assert!(!printed.contains("HOME"));

    let (pwd_path, pwd_hash) = bind_copy(dir.path(), "pwd", "where");
    let there = run(
        &pwd_path,
        &pwd_hash,
        &[],
        Limits::default(),
        TIMEOUT,
        work.path(),
    )
    .await;
    let canonical = work.path().canonicalize().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&there.stdout).trim(),
        canonical.to_str().unwrap()
    );
}

#[tokio::test]
async fn stdin_is_empty_so_a_program_waiting_for_input_ends() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "cat", "reader");
    let started = Instant::now();
    let result = run(&path, &hash, &[], Limits::default(), TIMEOUT, dir.path()).await;
    assert!(result.succeeded() && result.stdout.is_empty());
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn a_program_that_runs_too_long_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "sleep", "waiter");
    let started = Instant::now();
    let result = run(
        &path,
        &hash,
        &["--", "60"],
        Limits::default(),
        Duration::from_secs(1),
        dir.path(),
    )
    .await;
    assert!(result.timed_out);
    assert_eq!(result.signal, Some(9));
    assert!(!result.succeeded());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_program_that_floods_its_output_is_killed_and_the_capture_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "yes", "flood");
    let limits = Limits {
        max_output_bytes: 4096,
        ..Limits::default()
    };
    let started = Instant::now();
    let result = run(&path, &hash, &[], limits, TIMEOUT, dir.path()).await;
    assert!(result.output_limit_hit);
    assert!(result.stdout.len() + result.stderr.len() <= 4096);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

fn python() -> Option<std::path::PathBuf> {
    ["/usr/bin/python3", "/bin/python3"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
}

#[tokio::test]
async fn limits_and_privileges_are_applied_to_the_child() {
    let Some(_) = python() else {
        eprintln!("python3 not found; skipping the limits test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "python3", "probe");

    let status = run(
        &path,
        &hash,
        &["-c", "print(open('/proc/self/status').read())"],
        Limits::default(),
        TIMEOUT,
        dir.path(),
    )
    .await;
    let text = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(
        text.lines()
            .any(|l| l.starts_with("NoNewPrivs:") && l.ends_with('1')),
        "no_new_privs not set:\n{text}"
    );

    let small = Limits {
        memory_mb: 256,
        ..Limits::default()
    };
    let hog = run(
        &path,
        &hash,
        &["-c", "x = bytearray(700 * 1024 * 1024)"],
        small,
        TIMEOUT,
        dir.path(),
    )
    .await;
    assert_ne!(
        hog.exit_code,
        Some(0),
        "700 MiB must not fit in a 256 MiB address space"
    );
    assert!(
        String::from_utf8_lossy(&hog.stderr).contains("MemoryError"),
        "stderr: {}",
        String::from_utf8_lossy(&hog.stderr)
    );

    let big_file = run(
        &path,
        &hash,
        &["-c", "open('big','wb').write(bytes(5 * 1024 * 1024))"],
        Limits::default(),
        TIMEOUT,
        dir.path(),
    )
    .await;
    assert!(
        !big_file.succeeded(),
        "a 5 MiB file must exceed the 1 MiB file-size limit"
    );
}

fn alive(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
        && std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "))
}

async fn gone_within(pid: i32, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    !alive(pid)
}

#[tokio::test]
async fn grandchildren_are_killed_with_the_group_on_timeout() {
    if python().is_none() {
        eprintln!("python3 not found; skipping the process-group test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "python3", "spawner");
    let code = "import subprocess, sys, time\nc = subprocess.Popen(['sleep', '60'])\nprint(c.pid, flush=True)\ntime.sleep(60)";
    let result = run(
        &path,
        &hash,
        &["-c", code],
        Limits::default(),
        Duration::from_secs(1),
        dir.path(),
    )
    .await;
    assert!(result.timed_out);
    let pid: i32 = String::from_utf8_lossy(&result.stdout)
        .trim()
        .parse()
        .expect("grandchild pid");
    assert!(
        gone_within(pid, Duration::from_secs(3)).await,
        "the grandchild {pid} survived the timeout"
    );
}

#[tokio::test]
async fn a_background_process_left_behind_by_a_finished_program_is_killed_too() {
    if python().is_none() {
        eprintln!("python3 not found; skipping the orphan test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (path, hash) = bind_copy(dir.path(), "python3", "daemoniser");
    let code = "import subprocess\nc = subprocess.Popen(['sleep', '60'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\nprint(c.pid, flush=True)";
    let result = run(
        &path,
        &hash,
        &["-c", code],
        Limits::default(),
        TIMEOUT,
        dir.path(),
    )
    .await;
    assert!(result.succeeded(), "the program itself exits normally");
    let pid: i32 = String::from_utf8_lossy(&result.stdout)
        .trim()
        .parse()
        .expect("orphan pid");
    assert!(
        gone_within(pid, Duration::from_secs(3)).await,
        "the orphan {pid} survived"
    );
}
