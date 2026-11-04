//! The manifest: the contract of one capability that a person bound.
//!
//! The assistant reads manifests; it never writes one. Loading applies the checks that make a dangerous manifest
//! impossible to register, before anything runs.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

/// Whether running the command needs a person to say yes each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// Every run is approved by a person. The default, so a manifest that says nothing is the safe one.
    #[default]
    Always,
    /// Harmless, read-only commands that a person has decided can run without asking.
    Never,
}

/// The type of one positional argument. Each has a validator in [`crate::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgKind {
    /// Letters, digits and `._:-`, at most 128 characters, never starting with `-`.
    Token,
    /// One of a fixed list.
    Enum { values: Vec<String> },
    /// A TCP/UDP port, 1–65535.
    Port,
    /// An integer in a range.
    Int { min: i64, max: i64 },
    /// A fully qualified domain name.
    Fqdn,
    /// A dotted IPv4 address.
    Ipv4,
}

/// One positional argument. On disk it is flat (`{"name": "n", "type": "int", "min": 1, "max": 9, "position": 0}`);
/// serde cannot combine `flatten` with `deny_unknown_fields`, so the strict shape is checked in `TryFrom`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "ArgSpecFile", into = "ArgSpecFile")]
pub struct ArgSpec {
    pub name: String,
    pub kind: ArgKind,
    pub required: bool,
    /// Position among the positional arguments: 0, 1, 2… with no gaps.
    pub position: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArgSpecFile {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    required: bool,
    position: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max: Option<i64>,
}

impl TryFrom<ArgSpecFile> for ArgSpec {
    type Error = String;

    fn try_from(file: ArgSpecFile) -> Result<Self, String> {
        let ArgSpecFile {
            name,
            kind,
            required,
            position,
            values,
            min,
            max,
        } = file;
        let stray =
            |what: &str| format!("argument {name:?} of type {kind:?} does not take '{what}'");
        let kind_parsed = match kind.as_str() {
            "token" | "port" | "fqdn" | "ipv4" => {
                if values.is_some() {
                    return Err(stray("values"));
                }
                if min.is_some() || max.is_some() {
                    return Err(stray("min/max"));
                }
                match kind.as_str() {
                    "token" => ArgKind::Token,
                    "port" => ArgKind::Port,
                    "fqdn" => ArgKind::Fqdn,
                    _ => ArgKind::Ipv4,
                }
            }
            "enum" => {
                if min.is_some() || max.is_some() {
                    return Err(stray("min/max"));
                }
                ArgKind::Enum {
                    values: values
                        .ok_or_else(|| format!("enum argument {name:?} needs 'values'"))?,
                }
            }
            "int" => {
                if values.is_some() {
                    return Err(stray("values"));
                }
                match (min, max) {
                    (Some(min), Some(max)) => ArgKind::Int { min, max },
                    _ => return Err(format!("int argument {name:?} needs 'min' and 'max'")),
                }
            }
            other => return Err(format!("argument {name:?} has an unknown type {other:?}")),
        };
        Ok(Self {
            name,
            kind: kind_parsed,
            required,
            position,
        })
    }
}

impl From<ArgSpec> for ArgSpecFile {
    fn from(spec: ArgSpec) -> Self {
        let (kind, values, min, max) = match spec.kind {
            ArgKind::Token => ("token", None, None, None),
            ArgKind::Port => ("port", None, None, None),
            ArgKind::Fqdn => ("fqdn", None, None, None),
            ArgKind::Ipv4 => ("ipv4", None, None, None),
            ArgKind::Enum { values } => ("enum", Some(values), None, None),
            ArgKind::Int { min, max } => ("int", None, Some(min), Some(max)),
        };
        Self {
            name: spec.name,
            kind: kind.to_owned(),
            required: spec.required,
            position: spec.position,
            values,
            min,
            max,
        }
    }
}

/// Resource limits applied to the child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "Limits::default_cpu")]
    pub cpu_seconds: u64,
    #[serde(default = "Limits::default_memory")]
    pub memory_mb: u64,
    /// Combined stdout + stderr. A command that writes more is killed, not buffered.
    #[serde(default = "Limits::default_output")]
    pub max_output_bytes: usize,
}

impl Limits {
    fn default_cpu() -> u64 {
        10
    }
    fn default_memory() -> u64 {
        512
    }
    fn default_output() -> usize {
        64 * 1024
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu_seconds: Self::default_cpu(),
            memory_mb: Self::default_memory(),
            max_output_bytes: Self::default_output(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub description: String,
    /// Absolute path of a single-purpose executable.
    pub binary: String,
    /// SHA-256 of the executable, hex. A different file at that path is refused.
    pub binary_sha256: String,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
    /// The only flags the command may be given, each exactly as written here. Empty: no flags at all.
    #[serde(default)]
    pub allowed_flags: Vec<String>,
    pub timeout_seconds: u64,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub approval: Approval,
    /// Put `--` before the positional arguments. Right for programs that follow the getopt convention; wrong for those
    /// that print it (`echo`), which set this to `false`. The ban on a leading `-` in every positional argument holds
    /// either way, so a value can never be read as a flag whatever the program does with `--`.
    #[serde(default = "default_true")]
    pub end_of_options: bool,
}

fn default_true() -> bool {
    true
}

/// General-purpose programs. Allow-listing one of these is allow-listing everything it can do (a shell runs anything;
/// `find -exec`, `git -c core.sshCommand=`, `curl -o` and `python -c` all execute or write what the caller says), so a
/// manifest naming one is refused. Bind a narrow wrapper instead.
const GENERAL_PURPOSE: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "fish", "csh", "tcsh", "ash", "busybox", "env", "xargs",
    "find", "fd", "git", "python", "python2", "python3", "perl", "ruby", "node", "deno", "bun",
    "php", "lua", "go", "awk", "gawk", "sed", "ssh", "scp", "sftp", "nc", "ncat", "netcat",
    "socat", "telnet", "curl", "wget", "ftp", "rsync", "docker", "podman", "kubectl", "sudo",
    "doas", "su", "eval", "exec", "make", "cmake", "gdb", "strace", "ltrace", "rlwrap", "nohup",
    "timeout", "nice", "ionice", "chroot", "script", "tar", "unzip", "zip", "dd", "tee", "install",
    "cp", "mv", "rm",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("cannot read {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("{path} is not a valid manifest: {reason}")]
    Parse { path: String, reason: String },
    #[error("manifest file {0} is writable by group or others; make it read-only (chmod 444)")]
    WritableManifest(String),
    #[error("manifest id {0:?} must be 1-64 characters of a-z, 0-9, _ or -")]
    BadId(String),
    #[error("binary {0:?} is not an absolute path without '..'")]
    BinaryNotAbsolute(String),
    #[error("{0:?} is a general-purpose program; bind a narrow single-purpose wrapper instead")]
    GeneralPurposeBinary(String),
    #[error("binary_sha256 must be 64 hex characters")]
    BadHash,
    #[error(
        "flag {0:?} is not a plain switch (it must start with '-' and use only letters, digits, '-', '_', '.')"
    )]
    BadFlag(String),
    #[error("flag {0:?} is listed twice")]
    DuplicateFlag(String),
    #[error("argument positions must be 0, 1, 2… without gaps or repeats")]
    BadArgPositions,
    #[error("argument name {0:?} is empty, too long or repeated")]
    BadArgName(String),
    #[error("enum value {0:?} is empty, longer than 128 characters or starts with '-'")]
    BadEnumValue(String),
    #[error("integer range {min}..={max} is empty")]
    BadRange { min: i64, max: i64 },
    #[error("timeout_seconds must be between 1 and 600")]
    BadTimeout,
    #[error("limits must be positive and max_output_bytes at most 4 MiB")]
    BadLimits,
    #[error("two manifests use the id {0:?}")]
    DuplicateId(String),
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn flag_ok(flag: &str) -> bool {
    flag.len() >= 2
        && flag.len() <= 64
        && flag.starts_with('-')
        && flag != "--"
        && flag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

impl Manifest {
    /// Parse and check a manifest from text.
    ///
    /// # Errors
    /// Malformed JSON, unknown fields, or any of the safety checks in [`Manifest::check`].
    pub fn parse(text: &str, origin: &str) -> Result<Self, ManifestError> {
        let manifest: Manifest = serde_json::from_str(text).map_err(|e| ManifestError::Parse {
            path: origin.to_owned(),
            reason: e.to_string(),
        })?;
        manifest.check()?;
        Ok(manifest)
    }

    /// Read a manifest file. It must not be writable by group or others.
    ///
    /// # Errors
    /// I/O failure, a writable file, or any parse/safety error.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let shown = path.display().to_string();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_err(|e| ManifestError::Io {
                    path: shown.clone(),
                    reason: e.to_string(),
                })?
                .permissions()
                .mode();
            if mode & 0o022 != 0 {
                return Err(ManifestError::WritableManifest(shown));
            }
        }
        let text = std::fs::read_to_string(path).map_err(|e| ManifestError::Io {
            path: shown.clone(),
            reason: e.to_string(),
        })?;
        Self::parse(&text, &shown)
    }

    /// The safety checks. They hold for a manifest read from disk and for one built in code.
    ///
    /// # Errors
    /// The first violated rule.
    pub fn check(&self) -> Result<(), ManifestError> {
        if !id_ok(&self.id) {
            return Err(ManifestError::BadId(self.id.clone()));
        }
        let binary = Path::new(&self.binary);
        if !binary.is_absolute()
            || binary
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(ManifestError::BinaryNotAbsolute(self.binary.clone()));
        }
        let base = binary
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let stem = base.split(['.', '-']).next().unwrap_or_default();
        if GENERAL_PURPOSE.contains(&base.as_str()) || GENERAL_PURPOSE.contains(&stem) {
            return Err(ManifestError::GeneralPurposeBinary(base));
        }
        let hash = self.binary_sha256.trim();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ManifestError::BadHash);
        }
        let mut seen_flags = BTreeSet::new();
        for flag in &self.allowed_flags {
            if !flag_ok(flag) {
                return Err(ManifestError::BadFlag(flag.clone()));
            }
            if !seen_flags.insert(flag.as_str()) {
                return Err(ManifestError::DuplicateFlag(flag.clone()));
            }
        }
        let mut positions: Vec<u8> = self.args.iter().map(|a| a.position).collect();
        positions.sort_unstable();
        if positions
            .iter()
            .enumerate()
            .any(|(i, p)| usize::from(*p) != i)
        {
            return Err(ManifestError::BadArgPositions);
        }
        let mut names = BTreeSet::new();
        for arg in &self.args {
            if arg.name.is_empty() || arg.name.len() > 64 || !names.insert(arg.name.as_str()) {
                return Err(ManifestError::BadArgName(arg.name.clone()));
            }
            match &arg.kind {
                ArgKind::Enum { values } => {
                    for v in values {
                        if v.is_empty() || v.len() > 128 || v.starts_with('-') || v.contains('\0') {
                            return Err(ManifestError::BadEnumValue(v.clone()));
                        }
                    }
                }
                ArgKind::Int { min, max } if min > max => {
                    return Err(ManifestError::BadRange {
                        min: *min,
                        max: *max,
                    });
                }
                _ => {}
            }
        }
        if !(1..=600).contains(&self.timeout_seconds) {
            return Err(ManifestError::BadTimeout);
        }
        let l = &self.limits;
        if l.cpu_seconds == 0
            || l.memory_mb == 0
            || l.max_output_bytes == 0
            || l.max_output_bytes > 4 << 20
        {
            return Err(ManifestError::BadLimits);
        }
        Ok(())
    }
}

/// Load every `*.json` in a directory. Ids must be unique.
///
/// # Errors
/// The directory cannot be read, a manifest is rejected, or two share an id.
pub fn load_dir(dir: &Path) -> Result<Vec<Manifest>, ManifestError> {
    let shown = dir.display().to_string();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| ManifestError::Io {
            path: shown.clone(),
            reason: e.to_string(),
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut out: Vec<Manifest> = Vec::new();
    for path in paths {
        let manifest = Manifest::load(&path)?;
        if out.iter().any(|m| m.id == manifest.id) {
            return Err(ManifestError::DuplicateId(manifest.id));
        }
        out.push(manifest);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            id: "say".into(),
            version: "1.0.0".into(),
            description: "print a word".into(),
            binary: "/usr/bin/echo".into(),
            binary_sha256: "ab".repeat(32),
            args: vec![ArgSpec {
                name: "word".into(),
                kind: ArgKind::Token,
                required: true,
                position: 0,
            }],
            allowed_flags: vec![],
            timeout_seconds: 5,
            limits: Limits::default(),
            approval: Approval::Always,
            end_of_options: true,
        }
    }

    #[test]
    fn a_sound_manifest_passes() {
        assert_eq!(sample().check(), Ok(()));
    }

    #[test]
    fn approval_defaults_to_always() {
        let json = r#"{"id":"x","version":"1","description":"d","binary":"/usr/bin/echo","binary_sha256":"%H","timeout_seconds":3}"#
            .replace("%H", &"0".repeat(64));
        assert_eq!(
            Manifest::parse(&json, "t").expect("parses").approval,
            Approval::Always
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = r#"{"id":"x","version":"1","description":"d","binary":"/usr/bin/echo","binary_sha256":"%H","timeout_seconds":3,"sudo":true}"#
            .replace("%H", &"0".repeat(64));
        assert!(matches!(
            Manifest::parse(&json, "t"),
            Err(ManifestError::Parse { .. })
        ));
    }

    #[test]
    fn general_purpose_programs_are_refused_by_name_and_by_stem() {
        for binary in [
            "/bin/sh",
            "/usr/bin/python3",
            "/usr/bin/python3.12",
            "/usr/bin/curl",
            "/usr/bin/find",
            "/opt/x/BASH",
            "/usr/bin/git",
        ] {
            let m = Manifest {
                binary: binary.into(),
                ..sample()
            };
            assert!(
                matches!(m.check(), Err(ManifestError::GeneralPurposeBinary(_))),
                "accepted {binary}"
            );
        }
    }

    #[test]
    fn the_binary_must_be_an_absolute_path_without_dots() {
        for binary in ["echo", "./echo", "/usr/bin/../bin/echo", "bin/echo"] {
            let m = Manifest {
                binary: binary.into(),
                ..sample()
            };
            assert!(
                matches!(m.check(), Err(ManifestError::BinaryNotAbsolute(_))),
                "accepted {binary}"
            );
        }
    }

    #[test]
    fn a_malformed_hash_is_refused() {
        for hash in ["", "abc", &"g".repeat(64), &"a".repeat(63), &"a".repeat(65)] {
            let m = Manifest {
                binary_sha256: hash.to_owned(),
                ..sample()
            };
            assert_eq!(m.check(), Err(ManifestError::BadHash), "accepted {hash:?}");
        }
    }

    #[test]
    fn flags_must_be_plain_switches_and_unique() {
        for flag in ["v", "--", "-", "-x y", "--out=/etc/passwd", "-;ls", "--a$b"] {
            let m = Manifest {
                allowed_flags: vec![flag.into()],
                ..sample()
            };
            assert!(
                matches!(m.check(), Err(ManifestError::BadFlag(_))),
                "accepted {flag:?}"
            );
        }
        let dup = Manifest {
            allowed_flags: vec!["-a".into(), "-a".into()],
            ..sample()
        };
        assert!(matches!(dup.check(), Err(ManifestError::DuplicateFlag(_))));
    }

    #[test]
    fn argument_positions_must_be_contiguous() {
        let mut m = sample();
        m.args.push(ArgSpec {
            name: "n".into(),
            kind: ArgKind::Port,
            required: false,
            position: 2,
        });
        assert_eq!(m.check(), Err(ManifestError::BadArgPositions));
        m.args[1].position = 0;
        assert_eq!(m.check(), Err(ManifestError::BadArgPositions));
    }

    #[test]
    fn enum_values_cannot_look_like_flags() {
        let mut m = sample();
        m.args[0].kind = ArgKind::Enum {
            values: vec!["ok".into(), "-rf".into()],
        };
        assert!(matches!(m.check(), Err(ManifestError::BadEnumValue(_))));
    }

    #[test]
    fn argument_specs_are_strict_about_their_shape() {
        let parse = |arg: &str| {
            let json = r#"{"id":"x","version":"1","description":"d","binary":"/usr/bin/echo","binary_sha256":"%H","timeout_seconds":3,"args":[%A]}"#
                .replace("%H", &"0".repeat(64))
                .replace("%A", arg);
            Manifest::parse(&json, "t")
        };
        assert!(parse(r#"{"name":"n","type":"int","min":1,"max":9,"position":0}"#).is_ok());
        assert!(parse(r#"{"name":"n","type":"enum","values":["a"],"position":0}"#).is_ok());
        for bad in [
            r#"{"name":"n","type":"token","position":0,"sudo":true}"#,
            r#"{"name":"n","type":"token","values":["a"],"position":0}"#,
            r#"{"name":"n","type":"int","min":1,"position":0}"#,
            r#"{"name":"n","type":"enum","position":0}"#,
            r#"{"name":"n","type":"shell","position":0}"#,
            r#"{"name":"n","type":"port","min":1,"max":2,"position":0}"#,
        ] {
            assert!(
                matches!(parse(bad), Err(ManifestError::Parse { .. })),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn ids_timeouts_and_limits_are_bounded() {
        assert!(matches!(
            Manifest {
                id: "Has Space".into(),
                ..sample()
            }
            .check(),
            Err(ManifestError::BadId(_))
        ));
        assert!(matches!(
            Manifest {
                id: String::new(),
                ..sample()
            }
            .check(),
            Err(ManifestError::BadId(_))
        ));
        assert_eq!(
            Manifest {
                timeout_seconds: 0,
                ..sample()
            }
            .check(),
            Err(ManifestError::BadTimeout)
        );
        assert_eq!(
            Manifest {
                timeout_seconds: 601,
                ..sample()
            }
            .check(),
            Err(ManifestError::BadTimeout)
        );
        let huge = Limits {
            max_output_bytes: 5 << 20,
            ..Limits::default()
        };
        assert_eq!(
            Manifest {
                limits: huge,
                ..sample()
            }
            .check(),
            Err(ManifestError::BadLimits)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_manifest_file_others_can_write_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("m.json");
        let json = serde_json::to_string(&sample()).expect("serialise");
        std::fs::write(&path, json).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("chmod");
        assert!(matches!(
            Manifest::load(&path),
            Err(ManifestError::WritableManifest(_))
        ));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");
        assert!(Manifest::load(&path).is_ok());
    }

    #[test]
    fn duplicate_ids_in_a_directory_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        for name in ["a.json", "b.json"] {
            let path = dir.path().join(name);
            std::fs::write(&path, serde_json::to_string(&sample()).expect("serialise"))
                .expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");
        }
        assert_eq!(
            load_dir(dir.path()),
            Err(ManifestError::DuplicateId("say".into()))
        );
    }
}
