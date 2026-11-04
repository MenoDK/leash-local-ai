//! Argument validation and `argv` construction.
//!
//! Three layers against argument injection (CWE-88): no positional argument may start with `-`; the vector always
//! carries a `--` before the positionals; and a flag is accepted only if the manifest lists it, byte for byte. There is
//! no shell anywhere, so metacharacters have no meaning, and the type checks refuse them anyway.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use crate::manifest::{ArgKind, ArgSpec, Manifest};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidateError {
    #[error("argument {0:?} is not declared by the manifest")]
    UnknownArg(String),
    #[error("required argument {0:?} is missing")]
    MissingRequired(String),
    #[error("argument {name:?} is not a valid {expected} (got {shown:?})")]
    BadValue {
        name: String,
        expected: &'static str,
        shown: String,
    },
    #[error("argument {0:?} starts with '-' and would be read as a flag")]
    LeadingDash(String),
    #[error("flag {0:?} is not on the manifest's allow-list")]
    FlagNotAllowed(String),
    #[error("flag {0:?} was given twice")]
    DuplicateFlag(String),
    #[error("argument {0:?} was given but an earlier optional argument was left out")]
    OutOfOrder(String),
}

/// A value, cut short and with control characters escaped, so an error message cannot carry a log-injection payload
/// or a megabyte of text.
fn shown(value: &str) -> String {
    let cut: String = value.chars().take(48).collect();
    cut.escape_debug().to_string()
}

fn fqdn_ok(s: &str) -> bool {
    s.len() <= 253
        && s.contains('.')
        && !s.ends_with('.')
        && s.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn token_ok(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

fn check_one(spec: &ArgSpec, value: &str) -> Result<(), ValidateError> {
    if value.starts_with('-') {
        return Err(ValidateError::LeadingDash(spec.name.clone()));
    }
    let (ok, expected) = match &spec.kind {
        ArgKind::Token => (token_ok(value), "token"),
        ArgKind::Enum { values } => (
            values.iter().any(|v| v == value),
            "one of the allowed values",
        ),
        // Digits only: `u16::from_str` would accept a leading '+', and `i64::from_str` a leading '+' as well.
        ArgKind::Port => (
            value.bytes().all(|b| b.is_ascii_digit()) && value.parse::<u16>().is_ok_and(|p| p >= 1),
            "port (1-65535)",
        ),
        ArgKind::Int { min, max } => (
            value.bytes().all(|b| b.is_ascii_digit())
                && !value.is_empty()
                && value
                    .parse::<i64>()
                    .is_ok_and(|n| (*min..=*max).contains(&n)),
            "integer in range",
        ),
        ArgKind::Fqdn => (fqdn_ok(value), "domain name"),
        // `Ipv4Addr::from_str` already rejects leading zeros, signs and whitespace.
        ArgKind::Ipv4 => (value.parse::<Ipv4Addr>().is_ok(), "IPv4 address"),
    };
    if ok {
        Ok(())
    } else {
        Err(ValidateError::BadValue {
            name: spec.name.clone(),
            expected,
            shown: shown(value),
        })
    }
}

/// Validate `args` and `flags` against the manifest and build the argument vector (without the program name):
/// `[flags…, "--", positionals…]` (the `--` only when the manifest asks for it).
///
/// # Errors
/// The first violation found.
pub fn build_argv(
    manifest: &Manifest,
    args: &BTreeMap<String, String>,
    flags: &[String],
) -> Result<Vec<String>, ValidateError> {
    for (i, flag) in flags.iter().enumerate() {
        if !manifest.allowed_flags.iter().any(|allowed| allowed == flag) {
            return Err(ValidateError::FlagNotAllowed(shown(flag)));
        }
        if flags[..i].contains(flag) {
            return Err(ValidateError::DuplicateFlag(flag.clone()));
        }
    }
    if let Some(unknown) = args
        .keys()
        .find(|name| !manifest.args.iter().any(|s| &s.name == *name))
    {
        return Err(ValidateError::UnknownArg(shown(unknown)));
    }

    let mut specs: Vec<&ArgSpec> = manifest.args.iter().collect();
    specs.sort_by_key(|s| s.position);

    let mut positionals = Vec::with_capacity(specs.len());
    let mut gap = false;
    for spec in specs {
        match args.get(&spec.name) {
            Some(_) if gap => return Err(ValidateError::OutOfOrder(spec.name.clone())),
            Some(value) => {
                check_one(spec, value)?;
                positionals.push(value.clone());
            }
            None if spec.required => return Err(ValidateError::MissingRequired(spec.name.clone())),
            // A skipped optional argument would shift the later ones into its place, so nothing may follow it.
            None => gap = true,
        }
    }

    let mut argv = Vec::with_capacity(flags.len() + positionals.len() + 1);
    argv.extend(flags.iter().cloned());
    if manifest.end_of_options {
        argv.push("--".to_owned());
    }
    argv.extend(positionals);
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Approval, Limits};

    fn manifest(args: Vec<ArgSpec>, flags: &[&str]) -> Manifest {
        Manifest {
            id: "t".into(),
            version: "1".into(),
            description: "t".into(),
            binary: "/usr/bin/echo".into(),
            binary_sha256: "0".repeat(64),
            args,
            allowed_flags: flags.iter().map(|f| (*f).to_owned()).collect(),
            timeout_seconds: 5,
            limits: Limits::default(),
            approval: Approval::Always,
            end_of_options: true,
        }
    }

    fn spec(name: &str, kind: ArgKind, required: bool, position: u8) -> ArgSpec {
        ArgSpec {
            name: name.into(),
            kind,
            required,
            position,
        }
    }

    fn one(kind: ArgKind, value: &str) -> Result<Vec<String>, ValidateError> {
        let m = manifest(vec![spec("v", kind, true, 0)], &[]);
        build_argv(
            &m,
            &BTreeMap::from([("v".to_owned(), value.to_owned())]),
            &[],
        )
    }

    #[test]
    fn builds_flags_then_separator_then_positionals() {
        let m = manifest(
            vec![
                spec("host", ArgKind::Fqdn, true, 0),
                spec("port", ArgKind::Port, false, 1),
            ],
            &["-v"],
        );
        let args = BTreeMap::from([
            ("host".to_owned(), "a.example.test".to_owned()),
            ("port".to_owned(), "443".to_owned()),
        ]);
        assert_eq!(
            build_argv(&m, &args, &["-v".to_owned()]),
            Ok(vec![
                "-v".into(),
                "--".into(),
                "a.example.test".into(),
                "443".into()
            ])
        );
    }

    #[test]
    fn the_separator_can_be_left_out_for_programs_that_would_print_it() {
        let mut m = manifest(vec![spec("w", ArgKind::Token, true, 0)], &[]);
        m.end_of_options = false;
        let args = BTreeMap::from([("w".to_owned(), "hello".to_owned())]);
        assert_eq!(build_argv(&m, &args, &[]), Ok(vec!["hello".to_owned()]));
        let dashed = BTreeMap::from([("w".to_owned(), "-n".to_owned())]);
        assert!(
            matches!(
                build_argv(&m, &dashed, &[]),
                Err(ValidateError::LeadingDash(_))
            ),
            "the dash ban does not depend on the separator"
        );
    }

    #[test]
    fn a_value_that_starts_with_a_dash_is_refused_for_every_type() {
        for kind in [
            ArgKind::Token,
            ArgKind::Fqdn,
            ArgKind::Port,
            ArgKind::Ipv4,
            ArgKind::Int { min: -5, max: 5 },
        ] {
            assert!(matches!(
                one(kind, "-1"),
                Err(ValidateError::LeadingDash(_))
            ));
        }
        assert!(matches!(
            one(ArgKind::Token, "--output=/etc/passwd"),
            Err(ValidateError::LeadingDash(_))
        ));
    }

    #[test]
    fn shell_metacharacters_never_pass_a_type_check() {
        for hostile in [
            "a; rm -rf /",
            "$(id)",
            "`id`",
            "a|b",
            "a&b",
            "a>b",
            "a b",
            "a\nb",
            "a\0b",
            "../../etc/passwd",
            "a'b",
            "a\"b",
            "~root",
            "*",
            "{a,b}",
        ] {
            assert!(
                one(ArgKind::Token, hostile).is_err(),
                "token accepted {hostile:?}"
            );
        }
        for hostile in [
            "a.com;id",
            "a.com/../x",
            "a..com",
            "-a.com",
            "a-.com",
            "a.com.",
            "nodot",
            &"a.".repeat(130),
        ] {
            assert!(
                one(ArgKind::Fqdn, hostile).is_err(),
                "fqdn accepted {hostile:?}"
            );
        }
    }

    #[test]
    fn numbers_are_digits_only_and_in_range() {
        assert!(one(ArgKind::Port, "443").is_ok());
        for bad in ["0", "65536", "+80", "8 0", "0x50", "1e3", "", "٨٠"] {
            assert!(one(ArgKind::Port, bad).is_err(), "port accepted {bad:?}");
        }
        assert!(one(ArgKind::Int { min: 1, max: 10 }, "10").is_ok());
        for bad in ["0", "11", "+5", "5.0", ""] {
            assert!(
                one(ArgKind::Int { min: 1, max: 10 }, bad).is_err(),
                "int accepted {bad:?}"
            );
        }
    }

    #[test]
    fn ipv4_rejects_the_forms_that_resolvers_read_differently() {
        assert!(one(ArgKind::Ipv4, "192.0.2.1").is_ok());
        for bad in [
            "192.0.2",
            "192.0.2.1.5",
            "0300.0.2.1",
            "3221225985",
            "192.0.2.256",
            "192.0.2.01",
            " 192.0.2.1",
        ] {
            assert!(one(ArgKind::Ipv4, bad).is_err(), "ipv4 accepted {bad:?}");
        }
    }

    #[test]
    fn an_enum_accepts_only_its_listed_values() {
        let kind = ArgKind::Enum {
            values: vec!["+%Y".into(), "+%s".into()],
        };
        assert!(one(kind.clone(), "+%s").is_ok());
        assert!(one(kind.clone(), "+%S").is_err());
        assert!(one(kind, "+%s ").is_err());
    }

    #[test]
    fn flags_are_matched_exactly_and_only_once() {
        let m = manifest(vec![], &["-s", "--verbose"]);
        let no_args = BTreeMap::new();
        assert!(build_argv(&m, &no_args, &["-s".into(), "--verbose".into()]).is_ok());
        for bad in ["-S", "--verbos", "-s=1", "--verbose ", "-sx"] {
            assert!(
                matches!(
                    build_argv(&m, &no_args, &[bad.into()]),
                    Err(ValidateError::FlagNotAllowed(_))
                ),
                "{bad:?}"
            );
        }
        assert!(matches!(
            build_argv(&m, &no_args, &["-s".into(), "-s".into()]),
            Err(ValidateError::DuplicateFlag(_))
        ));
    }

    #[test]
    fn a_later_argument_cannot_slide_into_a_skipped_optional_position() {
        let m = manifest(
            vec![
                spec("a", ArgKind::Token, false, 0),
                spec("b", ArgKind::Token, false, 1),
            ],
            &[],
        );
        let args = BTreeMap::from([("b".to_owned(), "two".to_owned())]);
        assert!(matches!(
            build_argv(&m, &args, &[]),
            Err(ValidateError::OutOfOrder(_))
        ));
    }

    #[test]
    fn undeclared_and_missing_arguments_are_refused() {
        let m = manifest(vec![spec("host", ArgKind::Fqdn, true, 0)], &[]);
        assert!(matches!(
            build_argv(&m, &BTreeMap::new(), &[]),
            Err(ValidateError::MissingRequired(_))
        ));
        let args = BTreeMap::from([
            ("host".to_owned(), "a.example.test".to_owned()),
            ("extra".to_owned(), "x".to_owned()),
        ]);
        assert!(matches!(
            build_argv(&m, &args, &[]),
            Err(ValidateError::UnknownArg(_))
        ));
    }

    #[test]
    fn error_messages_do_not_echo_a_hostile_value_verbatim() {
        let hostile = format!("line1\nFAKE LOG ENTRY {}", "x".repeat(10_000));
        let err = one(ArgKind::Token, &hostile)
            .expect_err("refused")
            .to_string();
        assert!(!err.contains('\n'), "a newline would forge a log line");
        assert!(err.len() < 300, "the message is bounded");
    }
}
