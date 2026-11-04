//! Shared helpers: bind real system programs by copying them into a temporary directory.
#![allow(dead_code, clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use leash_broker::{Approval, ArgKind, ArgSpec, Limits, Manifest, sha256_of};

/// Find a coreutils program in the usual places.
pub fn system_program(name: &str) -> PathBuf {
    ["/usr/bin", "/bin"]
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            panic!("{name} not found: the tests need coreutils and python3 on the PATH")
        })
}

/// Copy a system program into `dir` under `as_name`; returns its path and hash.
pub fn bind_copy(dir: &Path, program: &str, as_name: &str) -> (String, String) {
    let target = dir.join(as_name);
    std::fs::copy(system_program(program), &target).expect("copy");
    let hash = sha256_of(&target).expect("hash");
    (target.display().to_string(), hash)
}

pub fn manifest(
    id: &str,
    binary: &str,
    hash: &str,
    args: Vec<ArgSpec>,
    flags: &[&str],
    approval: Approval,
) -> Manifest {
    Manifest {
        id: id.into(),
        version: "1".into(),
        description: format!("test command {id}"),
        binary: binary.into(),
        binary_sha256: hash.into(),
        args,
        allowed_flags: flags.iter().map(|f| (*f).to_owned()).collect(),
        timeout_seconds: 10,
        limits: Limits::default(),
        approval,
        end_of_options: false,
    }
}

pub fn token(name: &str, position: u8) -> ArgSpec {
    ArgSpec {
        name: name.into(),
        kind: ArgKind::Token,
        required: true,
        position,
    }
}
