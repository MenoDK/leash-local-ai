//! Running one pinned executable, contained.
//!
//! - **Pinned by file descriptor.** The binary is opened once, hashed through that descriptor, and executed through
//!   `/proc/self/fd/N`. What runs is the file that was hashed, even if the path is swapped a microsecond later: there is
//!   no check-then-use gap on the path.
//! - **No shell.** The vector goes to `execve` as it is. Only compiled (ELF) programs are accepted: a script would be
//!   started by an interpreter named in the file, which is exactly the indirection the pin exists to remove.
//! - **A clean room.** Empty environment, an empty working directory, no stdin, no new privileges, CPU / memory / file
//!   size / descriptor limits, a process group that is killed as a whole on timeout, and output that is capped rather
//!   than buffered without limit.
//!
//! This is containment by narrowing, not a sandbox: there is no namespace, no seccomp and no Landlock here. The README
//! says so.

#[cfg(not(target_os = "linux"))]
compile_error!("leash-broker runs commands through /proc/self/fd and targets Linux");

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use leash_censor::{Limits as SealLimits, Provenance, SafeEnvelope, seal};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::manifest::Limits;

const MAX_BINARY_BYTES: u64 = 256 * 1024 * 1024;
/// After the child has exited, how long to keep reading a pipe that something else still holds open.
const DRAIN_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, thiserror::Error)]
pub enum PinError {
    #[error("cannot open {path}: {reason}")]
    Open { path: String, reason: String },
    #[error("{0} is not a regular file")]
    NotRegular(String),
    #[error("{0} is writable by group or others")]
    Writable(String),
    #[error(
        "{0} is not a compiled (ELF) program; scripts are started through an interpreter and cannot be pinned"
    )]
    NotElf(String),
    #[error("{0} is larger than the {MAX_BINARY_BYTES}-byte limit")]
    TooLarge(String),
    #[error("the file at {path} does not have the pinned hash")]
    HashMismatch { path: String },
}

/// An open, verified executable.
#[derive(Debug)]
pub struct PinnedBinary {
    file: File,
    name: String,
}

/// Hex SHA-256 of a file. Used by whoever *binds* a command (a person, or the demo's setup step) to write the manifest.
///
/// # Errors
/// The file cannot be read.
pub fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    hash_file(&mut file)
}

fn hash_file(file: &mut File) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

impl PinnedBinary {
    /// Open `path` without following a symlink in the last component, check it, and compare its hash.
    ///
    /// # Errors
    /// See [`PinError`].
    pub fn open(path: &str, expected_sha256: &str) -> Result<Self, PinError> {
        use rustix::fs::{Mode, OFlags};
        let open_err = |reason: String| PinError::Open {
            path: path.to_owned(),
            reason,
        };

        let fd = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| open_err(e.to_string()))?;
        let mut file = File::from(fd);
        let meta = file.metadata().map_err(|e| open_err(e.to_string()))?;
        if !meta.is_file() {
            return Err(PinError::NotRegular(path.to_owned()));
        }
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o022 != 0 {
                return Err(PinError::Writable(path.to_owned()));
            }
        }
        if meta.len() > MAX_BINARY_BYTES {
            return Err(PinError::TooLarge(path.to_owned()));
        }
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_err() || magic != *b"\x7fELF" {
            return Err(PinError::NotElf(path.to_owned()));
        }
        // Hash the whole file through this descriptor, from the start.
        rustix::fs::seek(&file, rustix::fs::SeekFrom::Start(0))
            .map_err(|e| open_err(e.to_string()))?;
        let actual = hash_file(&mut file).map_err(|e| open_err(e.to_string()))?;
        if !actual.eq_ignore_ascii_case(expected_sha256.trim()) {
            return Err(PinError::HashMismatch {
                path: path.to_owned(),
            });
        }
        let name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("program")
            .to_owned();
        Ok(Self { file, name })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("could not start the process: {0}")]
    Spawn(String),
    #[error("waiting for the process failed: {0}")]
    Wait(String),
}

/// What a run produced.
#[derive(Debug, Clone)]
pub struct Execution {
    pub exit_code: Option<i32>,
    /// Set when the process was ended by a signal (including our own kill).
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    /// The process wrote more than its limit and was killed.
    pub output_limit_hit: bool,
    pub duration_ms: u64,
}

impl Execution {
    /// The output as the model may see it: sealed as [`Provenance::ToolOutput`] (hostile by assumption).
    #[must_use]
    pub fn sealed_output(&self) -> SafeEnvelope {
        let mut text = String::from_utf8_lossy(&self.stdout).into_owned();
        if !self.stderr.is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(&String::from_utf8_lossy(&self.stderr));
        }
        seal(&text, Provenance::ToolOutput, SealLimits::default())
    }

    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.output_limit_hit
    }
}

fn kill_group(pid: u32) {
    if let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        // Ignore the error: the group may already be gone.
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
}

#[allow(unsafe_code)] // `pre_exec` is unsafe by signature; the closure below calls only async-signal-safe system calls.
fn confine(command: &mut Command, limits: Limits) {
    use rustix::process::{Resource, Rlimit, setrlimit};
    let cpu = limits.cpu_seconds;
    let memory = limits.memory_mb.saturating_mul(1024 * 1024);
    // SAFETY: the closure runs between fork and exec in the child. It allocates nothing and calls only prctl and
    // setrlimit, both async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            rustix::thread::set_no_new_privs(true).map_err(std::io::Error::from)?;
            let cap = |resource, value: u64| {
                setrlimit(
                    resource,
                    Rlimit {
                        current: Some(value),
                        maximum: Some(value),
                    },
                )
                .map_err(std::io::Error::from)
            };
            cap(Resource::Cpu, cpu)?;
            cap(Resource::As, memory)?;
            cap(Resource::Nofile, 64)?;
            cap(Resource::Fsize, 1 << 20)?;
            cap(Resource::Core, 0)?;
            Ok(())
        });
    }
}

async fn read_some<R: AsyncReadExt + Unpin>(
    pipe: &mut Option<R>,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    match pipe {
        Some(reader) => reader.read(buf).await,
        None => std::future::pending().await,
    }
}

/// Run the pinned program with `argv` (without the program name) in `workdir`.
///
/// # Errors
/// The process could not be started or waited for. A non-zero exit, a timeout and an output flood are *results*
/// (see [`Execution`]), not errors.
pub async fn run(
    binary: &PinnedBinary,
    argv: &[String],
    limits: Limits,
    timeout: Duration,
    workdir: &Path,
) -> Result<Execution, ExecError> {
    let proc_path = format!("/proc/self/fd/{}", binary.file.as_raw_fd());
    let mut command = Command::new(proc_path);
    command
        .arg0(&binary.name)
        .args(argv)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    confine(&mut command, limits);

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| ExecError::Spawn(e.to_string()))?;
    let pid = child.id().unwrap_or(0);
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();

    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let (mut out_buf, mut err_buf) = ([0u8; 8192], [0u8; 8192]);
    let mut status = None;
    let (mut timed_out, mut flooded) = (false, false);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let drain = tokio::time::sleep(Duration::from_secs(86_400));
    tokio::pin!(drain);
    let mut draining = false;

    while status.is_none() || out_pipe.is_some() || err_pipe.is_some() {
        tokio::select! {
            read = read_some(&mut out_pipe, &mut out_buf) => match read {
                Ok(n) if n > 0 => stdout.extend_from_slice(&out_buf[..n]),
                _ => out_pipe = None,
            },
            read = read_some(&mut err_pipe, &mut err_buf) => match read {
                Ok(n) if n > 0 => stderr.extend_from_slice(&err_buf[..n]),
                _ => err_pipe = None,
            },
            waited = child.wait(), if status.is_none() => {
                status = Some(waited.map_err(|e| ExecError::Wait(e.to_string()))?);
                draining = true;
                drain.as_mut().reset(tokio::time::Instant::now() + DRAIN_GRACE);
            },
            () = &mut deadline, if !timed_out && status.is_none() => {
                timed_out = true;
                kill_group(pid);
            },
            () = &mut drain, if draining => break,
        }
        let cap = limits.max_output_bytes;
        if stdout.len() + stderr.len() > cap {
            if !flooded {
                flooded = true;
                kill_group(pid);
            }
            // Keep at most `cap` bytes in total; whatever arrives after the kill is dropped here.
            stdout.truncate(cap);
            stderr.truncate(cap - stdout.len());
        }
    }
    // Whatever is left of the group (a grandchild that kept a pipe) goes with it.
    kill_group(pid);

    let status = match status {
        Some(s) => s,
        None => child
            .wait()
            .await
            .map_err(|e| ExecError::Wait(e.to_string()))?,
    };
    use std::os::unix::process::ExitStatusExt;
    Ok(Execution {
        exit_code: status.code(),
        signal: status.signal(),
        stdout,
        stderr,
        timed_out,
        output_limit_hit: flooded,
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}
