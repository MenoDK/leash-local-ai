//! Secret redaction.
//!
//! Conservative on purpose: a false positive costs one unreadable word, a false negative costs a credential in a
//! prompt. The patterns cover well-known shapes (private-key blocks, prefixed provider tokens, JWTs, credentials in
//! URLs, `NAME=value` assignments). They do **not** find an arbitrary high-entropy string with no telltale shape;
//! that is listed under limitations in the README.

use std::sync::OnceLock;

use regex::{Captures, Regex};

/// What replaces a redacted value.
pub const REDACTED: &str = "[REDACTED]";

struct Rules {
    /// The whole match is replaced.
    whole: Vec<Regex>,
    /// Only capture group 1 (the value) is replaced, so `API_KEY=[REDACTED]` still tells the reader what was there.
    value: Vec<Regex>,
}

#[allow(clippy::expect_used)] // constant patterns, exercised by the tests below
fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        let compile = |patterns: &[&str]| -> Vec<Regex> {
            patterns.iter().map(|p| Regex::new(p).expect("static pattern")).collect()
        };
        Rules {
            whole: compile(&[
                // A private key block, and the unterminated form (a truncated paste still leaks the key body).
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*",
                // Provider-prefixed tokens.
                r"\b(?:sk|pk|rk)[-_](?:live|test|proj|ant)?[-_]?[A-Za-z0-9_-]{20,}",
                r"\bgsk_[A-Za-z0-9]{20,}",
                r"\bgh[pousr]_[A-Za-z0-9]{30,}",
                r"\bgithub_pat_[A-Za-z0-9_]{30,}",
                r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
                r"\bAIza[0-9A-Za-z_-]{35}\b",
                r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
                // JSON Web Tokens, including the unsigned `alg: none` form with an empty signature.
                r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*",
                // `Authorization: Bearer ...`
                r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}",
                // Credentials inside a URL: scheme://user:password@host
                r"\b[a-zA-Z][a-zA-Z0-9+.-]{1,20}://[^\s:/@]+:[^\s@/]+@",
            ]),
            value: compile(&[
                // NAME=value / name: "value" where the name says it is sensitive. Group 1 is the value.
                r#"(?i)[A-Za-z0-9_.-]*(?:key|secret|token|passw(?:or)?d|pwd|credential)[A-Za-z0-9_.-]*["']?\s*[:=]\s*["']?([^\s"',;]{6,})"#,
            ]),
        }
    })
}

/// Values that are obviously not secrets: placeholders, references to an environment, types.
fn is_placeholder(value: &str) -> bool {
    let v = value
        .trim_matches(|c| c == '"' || c == '\'')
        .to_ascii_lowercase();
    const STARTS: &[&str] = &[
        "<",
        "${",
        "{{",
        "$(",
        "$",
        "%",
        "your",
        "xxx",
        "changeme",
        "change-me",
        "example",
        "placeholder",
        "dummy",
        "***",
        "[redacted]",
        "null",
        "none",
        "true",
        "false",
        "undefined",
        "string",
        "process.env",
        "os.environ",
        "env(",
        "getenv",
        "secrets.",
        "vault:",
        "ref+",
    ];
    STARTS.iter().any(|p| v.starts_with(p))
}

/// Replace every secret in `text`; returns the new text and how many values were replaced.
pub fn redact(text: &str) -> (String, usize) {
    let rules = rules();
    let mut out = text.to_owned();
    let mut count = 0;

    for re in &rules.whole {
        out = re
            .replace_all(&out, |caps: &Captures| {
                let whole = caps.get(0).map_or("", |m| m.as_str());
                if whole.contains(REDACTED) {
                    whole.to_owned()
                } else {
                    count += 1;
                    REDACTED.to_owned()
                }
            })
            .into_owned();
    }

    for re in &rules.value {
        out = re
            .replace_all(&out, |caps: &Captures| {
                let whole = caps.get(0).map_or("", |m| m.as_str());
                let value = caps.get(1).map_or("", |m| m.as_str());
                if value.is_empty() || is_placeholder(value) {
                    return whole.to_owned();
                }
                count += 1;
                whole.replacen(value, REDACTED, 1)
            })
            .into_owned();
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Secrets are assembled at run time so that no literal in this file looks like a real credential to a scanner.
    fn fake(prefix: &str, body_len: usize) -> String {
        format!(
            "{prefix}{}",
            "a1B2c3D4e5"
                .repeat(body_len.div_ceil(10))
                .chars()
                .take(body_len)
                .collect::<String>()
        )
    }

    #[test]
    fn redacts_prefixed_provider_tokens() {
        for token in [
            fake("sk-", 32),
            fake("ghp_", 36),
            fake("gsk_", 40),
            fake("xoxb-", 24),
            fake("AKIA", 16).to_uppercase(),
        ] {
            let (out, n) = redact(&format!("use {token} now"));
            assert_eq!(n, 1, "not redacted: shape {}", &token[..4]);
            assert!(!out.contains(&token));
        }
    }

    #[test]
    fn redacts_private_key_blocks_even_when_truncated() {
        let begin = format!("-----BEGIN {} PRIVATE KEY-----", "RSA");
        let end = format!("-----END {} PRIVATE KEY-----", "RSA");
        let (full, n_full) = redact(&format!("{begin}\nMIIEvQ\n{end}\nafter"));
        assert_eq!((n_full, full.as_str()), (1, "[REDACTED]\nafter"));

        let (cut, n_cut) = redact(&format!("{begin}\nMIIEvQabcdef"));
        assert_eq!((n_cut, cut.as_str()), (1, "[REDACTED]"));
    }

    #[test]
    fn redacts_jwts_including_empty_signature() {
        let jwt = format!("eyJ{}.{}.", "hbGciOiJub25lIn0", "eyJzdWIiOiIxIn0abcdef");
        let (out, n) = redact(&format!("t={jwt}"));
        assert!(n >= 1 && !out.contains("eyJ"));
    }

    #[test]
    fn redacts_url_credentials_but_keeps_plain_urls() {
        let (out, n) = redact(
            "postgres://svc:hunter22hunter@db.example.test:5432/app and https://example.test/path",
        );
        assert_eq!(n, 1);
        assert!(out.contains("https://example.test/path"));
        assert!(!out.contains("hunter22hunter"));
    }

    #[test]
    fn redacts_the_value_of_a_named_assignment_and_keeps_the_name() {
        let (out, n) = redact("DATABASE_PASSWORD=correct-horse-battery\napi_key: \"abcdef123456\"");
        assert_eq!(n, 2);
        assert_eq!(out, "DATABASE_PASSWORD=[REDACTED]\napi_key: \"[REDACTED]\"");
    }

    #[test]
    fn leaves_placeholders_and_references_alone() {
        let text = "API_KEY=<your-key>\nTOKEN=${TOKEN}\nSECRET=changeme\npassword: process.env.PASSWORD\nmonkey=bananas123";
        let (out, n) = redact(text);
        // `monkey=bananas123` contains "key" and looks like an assignment: this is the accepted false positive.
        assert_eq!(n, 1);
        assert!(out.starts_with(
            "API_KEY=<your-key>\nTOKEN=${TOKEN}\nSECRET=changeme\npassword: process.env.PASSWORD"
        ));
    }

    #[test]
    fn redaction_is_idempotent() {
        let once = redact(&format!("k={}", fake("ghp_", 36))).0;
        let (twice, n) = redact(&once);
        assert_eq!((once.as_str(), n), (twice.as_str(), 0));
    }

    #[test]
    fn ordinary_prose_and_code_pass_through_untouched() {
        let text = "The tokenizer splits words. Use `cargo test`. Key points: a, b, c. See https://example.test/docs.";
        assert_eq!(redact(text), (text.to_owned(), 0));
    }
}
