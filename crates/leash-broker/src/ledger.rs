//! The ledger: an append-only, hash-chained record of everything the broker decided and did.
//!
//! Each line is a JSON object; its `hash` covers every field and the previous entry's hash, so changing, removing or
//! reordering an entry breaks the chain from that point on. The encoding that is hashed is not the JSON text: it is
//! length-prefixed fields in a fixed order, so no two different entries can produce the same input to the hash.
//!
//! With a key the hash is an HMAC-SHA-256, so someone who can rewrite the file but does not hold the key cannot
//! recompute a consistent chain. Without a key, a full rewrite is only detectable against an anchor kept elsewhere
//! (the length and head hash; see [`check_anchor`]). Either way, deleting the *tail* is only detectable against such an
//! anchor: a shorter chain is still a valid chain.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Hash of the entry before the first one.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A secret that turns the chain into a keyed one. Never printed.
#[derive(Clone)]
pub struct LedgerKey(Vec<u8>);

impl LedgerKey {
    /// # Errors
    /// The key is shorter than 16 bytes.
    pub fn new(bytes: &[u8]) -> Result<Self, LedgerError> {
        if bytes.len() < 16 {
            return Err(LedgerError::WeakKey);
        }
        Ok(Self(bytes.to_vec()))
    }
}

impl std::fmt::Debug for LedgerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LedgerKey(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A request was accepted and waits for a decision.
    Proposed,
    /// A request was turned down before any decision (unknown command, bad argument, binary changed…).
    Refused,
    Approved,
    Denied,
    Expired,
    /// Written **before** the process is created. Without a later `Finished`, the run is *uncertain*.
    Started,
    Finished,
}

/// What the caller wants recorded.
#[derive(Debug, Clone)]
pub struct Record {
    pub kind: Kind,
    pub command_id: String,
    pub proposal_id: Option<String>,
    pub binary_sha256: Option<String>,
    pub detail: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub ts_ms: u64,
    pub kind: Kind,
    pub command_id: String,
    pub proposal_id: Option<String>,
    pub binary_sha256: Option<String>,
    pub detail: Value,
    pub prev: String,
    pub hash: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("ledger I/O: {0}")]
    Io(String),
    #[error("another process holds the ledger")]
    Locked,
    #[error("the ledger is broken at entry {seq}: {reason}")]
    Broken { seq: u64, reason: &'static str },
    #[error("a ledger key must be at least 16 bytes")]
    WeakKey,
}

fn io(e: &impl std::fmt::Display) -> LedgerError {
    LedgerError::Io(e.to_string())
}

/// JSON with object keys sorted, no whitespace. The hash does not depend on how a library orders keys.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(v) = map.get(key) {
                    canonical(v, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    buf.extend_from_slice(bytes);
}

fn optional(buf: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(v) => {
            buf.push(1);
            field(buf, v.as_bytes());
        }
        None => buf.push(0),
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Proposed => "proposed",
        Kind::Refused => "refused",
        Kind::Approved => "approved",
        Kind::Denied => "denied",
        Kind::Expired => "expired",
        Kind::Started => "started",
        Kind::Finished => "finished",
    }
}

fn entry_hash(key: Option<&LedgerKey>, e: &Entry) -> String {
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(b"leash-ledger-v1");
    field(&mut buf, &e.seq.to_be_bytes());
    field(&mut buf, &e.ts_ms.to_be_bytes());
    field(&mut buf, kind_name(e.kind).as_bytes());
    field(&mut buf, e.command_id.as_bytes());
    optional(&mut buf, e.proposal_id.as_deref());
    optional(&mut buf, e.binary_sha256.as_deref());
    let mut detail = String::new();
    canonical(&e.detail, &mut detail);
    field(&mut buf, detail.as_bytes());
    field(&mut buf, e.prev.as_bytes());
    match key {
        Some(LedgerKey(secret)) => match HmacSha256::new_from_slice(secret) {
            Ok(mut mac) => {
                mac.update(&buf);
                hex::encode(mac.finalize().into_bytes())
            }
            // `new_from_slice` accepts any length for HMAC; this arm only exists so the function cannot panic.
            Err(_) => String::new(),
        },
        None => hex::encode(Sha256::digest(&buf)),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Where a chain stops being valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Break {
    pub seq: u64,
    pub reason: &'static str,
}

/// What reading a ledger found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verification {
    /// Entries that form a valid chain from the start.
    pub entries: u64,
    /// Hash of the last valid entry (the anchor to keep elsewhere).
    pub head: String,
    pub broken: Option<Break>,
    /// Proposals with a `started` entry and no `finished`: they may or may not have run.
    pub unfinished: Vec<String>,
}

impl Verification {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.broken.is_none()
    }
}

/// Walk the file and check every link. Never fails: a problem is reported in [`Verification::broken`].
#[must_use]
pub fn verify(path: &Path, key: Option<&LedgerKey>) -> Verification {
    let mut report = Verification {
        entries: 0,
        head: GENESIS.to_owned(),
        broken: None,
        unfinished: Vec::new(),
    };
    let Ok(file) = File::open(path) else {
        return report;
    };
    verify_reader(BufReader::new(file), key, &mut report);
    report
}

fn verify_reader(mut reader: impl BufRead, key: Option<&LedgerKey>, report: &mut Verification) {
    let mut started: Vec<String> = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => {
                report.broken = Some(Break {
                    seq: report.entries,
                    reason: "unreadable",
                });
                break;
            }
        }
        if !line.ends_with('\n') {
            report.broken = Some(Break {
                seq: report.entries,
                reason: "torn last line (a write was cut short)",
            });
            break;
        }
        let Ok(entry) = serde_json::from_str::<Entry>(line.trim_end()) else {
            report.broken = Some(Break {
                seq: report.entries,
                reason: "not a ledger entry",
            });
            break;
        };
        if entry.seq != report.entries {
            report.broken = Some(Break {
                seq: report.entries,
                reason: "sequence number out of order (entry removed or inserted)",
            });
            break;
        }
        if entry.prev != report.head {
            report.broken = Some(Break {
                seq: entry.seq,
                reason: "does not follow the previous entry",
            });
            break;
        }
        if entry_hash(key, &entry) != entry.hash {
            report.broken = Some(Break {
                seq: entry.seq,
                reason: "contents do not match the hash (edited)",
            });
            break;
        }
        match (entry.kind, &entry.proposal_id) {
            (Kind::Started, Some(id)) => started.push(id.clone()),
            (Kind::Finished, Some(id)) => started.retain(|s| s != id),
            _ => {}
        }
        report.head = entry.hash;
        report.entries += 1;
    }
    report.unfinished = started;
}

/// Why a ledger does not match a remembered anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorMismatch {
    /// The chain is shorter than it was: entries were cut off the end.
    Truncated { expected: u64, found: u64 },
    /// Same or longer, but the entry at the anchor has a different hash: history was rewritten.
    Rewritten,
}

/// Compare a verified ledger with a `(length, head)` pair recorded earlier somewhere the writer cannot reach.
///
/// # Errors
/// The ledger is shorter than the anchor, or its entry at the anchor's position differs.
pub fn check_anchor(
    path: &Path,
    key: Option<&LedgerKey>,
    length: u64,
    head: &str,
) -> Result<(), AnchorMismatch> {
    let report = verify(path, key);
    if report.entries < length {
        return Err(AnchorMismatch::Truncated {
            expected: length,
            found: report.entries,
        });
    }
    // Hash of the entry that was last at `length`: walk again, stopping there.
    let Ok(file) = File::open(path) else {
        return Err(AnchorMismatch::Rewritten);
    };
    let mut seen = 0u64;
    let mut at_anchor = GENESIS.to_owned();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if seen == length {
            break;
        }
        if let Ok(entry) = serde_json::from_str::<Entry>(&line) {
            at_anchor = entry.hash;
            seen += 1;
        }
    }
    if at_anchor == head {
        Ok(())
    } else {
        Err(AnchorMismatch::Rewritten)
    }
}

/// The writer. One per file: it holds an exclusive lock.
#[derive(Debug)]
pub struct Ledger {
    file: File,
    path: PathBuf,
    key: Option<LedgerKey>,
    next_seq: u64,
    head: String,
    /// Test seam: makes the next appends fail, to prove that nothing runs when the record cannot be written.
    #[cfg(test)]
    pub(crate) failing: bool,
}

impl Ledger {
    /// Open (or create) the ledger, take the exclusive lock, and refuse to extend a chain that does not verify.
    ///
    /// # Errors
    /// I/O failure, a second writer, or a broken chain.
    pub fn open(path: &Path, key: Option<LedgerKey>) -> Result<Self, LedgerError> {
        let mut options = OpenOptions::new();
        options.read(true).append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(|e| io(&e))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |e| match e {
                rustix::io::Errno::WOULDBLOCK => LedgerError::Locked,
                other => io(&other),
            },
        )?;

        file.seek(SeekFrom::Start(0)).map_err(|e| io(&e))?;
        let mut report = Verification {
            entries: 0,
            head: GENESIS.to_owned(),
            broken: None,
            unfinished: Vec::new(),
        };
        verify_reader(BufReader::new(&file), key.as_ref(), &mut report);
        if let Some(Break { seq, reason }) = report.broken {
            return Err(LedgerError::Broken { seq, reason });
        }
        Ok(Self {
            file,
            path: path.to_path_buf(),
            key,
            next_seq: report.entries,
            head: report.head,
            #[cfg(test)]
            failing: false,
        })
    }

    /// Add an entry and flush it to disk before returning.
    ///
    /// # Errors
    /// The write or the flush failed. The in-memory head does not move, so a retry continues the same chain.
    pub fn append(&mut self, record: Record) -> Result<Entry, LedgerError> {
        #[cfg(test)]
        if self.failing {
            return Err(LedgerError::Io("simulated write failure".to_owned()));
        }
        let mut entry = Entry {
            seq: self.next_seq,
            ts_ms: now_ms(),
            kind: record.kind,
            command_id: record.command_id,
            proposal_id: record.proposal_id,
            binary_sha256: record.binary_sha256,
            detail: record.detail,
            prev: self.head.clone(),
            hash: String::new(),
        };
        entry.hash = entry_hash(self.key.as_ref(), &entry);
        let mut line = serde_json::to_string(&entry).map_err(|e| io(&e))?;
        line.push('\n');
        self.file.write_all(line.as_bytes()).map_err(|e| io(&e))?;
        self.file.sync_data().map_err(|e| io(&e))?;
        self.next_seq += 1;
        self.head.clone_from(&entry.hash);
        Ok(entry)
    }

    #[must_use]
    pub fn head(&self) -> &str {
        &self.head
    }

    #[must_use]
    pub fn len(&self) -> u64 {
        self.next_seq
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.next_seq == 0
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn record(kind: Kind, id: &str) -> Record {
        Record {
            kind,
            command_id: "say".into(),
            proposal_id: Some(id.into()),
            binary_sha256: Some("ab".repeat(32)),
            detail: json!({"b": 2, "a": [1, {"z": null, "y": "x"}]}),
        }
    }

    fn filled(dir: &Path, key: Option<LedgerKey>, n: usize) -> PathBuf {
        let path = dir.join("ledger.jsonl");
        let mut ledger = Ledger::open(&path, key).expect("open");
        for i in 0..n {
            ledger
                .append(record(Kind::Proposed, &format!("p{i}")))
                .expect("append");
        }
        path
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .expect("read")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn write_lines(path: &Path, lines: &[String]) {
        std::fs::write(
            path,
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .expect("write");
    }

    fn key() -> LedgerKey {
        LedgerKey::new(&[7u8; 32]).expect("key")
    }

    #[test]
    fn an_untouched_chain_verifies_and_resumes_after_reopening() {
        let dir = tempfile::tempdir().expect("dir");
        let path = filled(dir.path(), None, 3);
        let report = verify(&path, None);
        assert_eq!((report.entries, report.is_valid()), (3, true));

        let mut again = Ledger::open(&path, None).expect("reopen");
        assert_eq!(again.len(), 3);
        again.append(record(Kind::Finished, "p0")).expect("append");
        assert_eq!(verify(&path, None).entries, 4);
    }

    #[test]
    fn editing_an_entry_breaks_the_chain_at_that_entry() {
        let dir = tempfile::tempdir().expect("dir");
        let path = filled(dir.path(), None, 4);
        let mut l = lines(&path);
        l[2] = l[2].replace("p2", "p9");
        write_lines(&path, &l);
        let report = verify(&path, None);
        assert_eq!(report.broken.map(|b| b.seq), Some(2));
        assert_eq!(
            report.entries, 2,
            "the two entries before the edit still count"
        );
    }

    #[test]
    fn removing_or_swapping_entries_is_detected() {
        let dir = tempfile::tempdir().expect("dir");
        let path = filled(dir.path(), None, 4);
        let original = lines(&path);

        let mut removed = original.clone();
        removed.remove(1);
        write_lines(&path, &removed);
        assert!(verify(&path, None).broken.is_some(), "removal");

        let mut swapped = original;
        swapped.swap(1, 2);
        write_lines(&path, &swapped);
        assert!(verify(&path, None).broken.is_some(), "reordering");
    }

    #[test]
    fn a_torn_last_line_is_reported_and_refuses_a_reopen() {
        let dir = tempfile::tempdir().expect("dir");
        let path = filled(dir.path(), None, 2);
        let mut text = std::fs::read_to_string(&path).expect("read");
        text.truncate(text.len() - 20);
        std::fs::write(&path, text).expect("write");
        assert_eq!(verify(&path, None).broken.map(|b| b.seq), Some(1));
        assert!(matches!(
            Ledger::open(&path, None),
            Err(LedgerError::Broken { seq: 1, .. })
        ));
    }

    #[test]
    fn an_unkeyed_chain_can_be_rewritten_consistently_but_a_keyed_one_cannot() {
        let dir = tempfile::tempdir().expect("dir");
        let plain = filled(dir.path(), None, 3);
        // The attacker edits an entry and recomputes every hash after it. No key is needed for a plain chain.
        let mut forged = Vec::new();
        let mut prev = GENESIS.to_owned();
        for (i, line) in lines(&plain).iter().enumerate() {
            let mut e: Entry = serde_json::from_str(line).expect("entry");
            if i == 1 {
                e.command_id = "something-else".into();
            }
            e.prev = prev.clone();
            e.hash = entry_hash(None, &e);
            prev = e.hash.clone();
            forged.push(serde_json::to_string(&e).expect("json"));
        }
        write_lines(&plain, &forged);
        assert!(
            verify(&plain, None).is_valid(),
            "documented limit: a plain chain proves integrity only against an anchor"
        );

        let keyed_dir = tempfile::tempdir().expect("dir");
        let keyed = filled(keyed_dir.path(), Some(key()), 3);
        let mut forged = Vec::new();
        let mut prev = GENESIS.to_owned();
        for (i, line) in lines(&keyed).iter().enumerate() {
            let mut e: Entry = serde_json::from_str(line).expect("entry");
            if i == 1 {
                e.command_id = "something-else".into();
            }
            e.prev = prev.clone();
            e.hash = entry_hash(None, &e); // the attacker has no key, so can only use the plain hash
            prev = e.hash.clone();
            forged.push(serde_json::to_string(&e).expect("json"));
        }
        write_lines(&keyed, &forged);
        assert!(
            !verify(&keyed, Some(&key())).is_valid(),
            "the keyed chain notices"
        );
    }

    #[test]
    fn an_anchor_catches_truncation_and_rewriting() {
        let dir = tempfile::tempdir().expect("dir");
        let path = filled(dir.path(), None, 5);
        let report = verify(&path, None);
        assert_eq!(check_anchor(&path, None, 5, &report.head), Ok(()));

        let kept: Vec<String> = lines(&path).into_iter().take(3).collect();
        write_lines(&path, &kept);
        assert!(
            verify(&path, None).is_valid(),
            "a shorter chain is still a valid chain"
        );
        assert_eq!(
            check_anchor(&path, None, 5, &report.head),
            Err(AnchorMismatch::Truncated {
                expected: 5,
                found: 3
            })
        );
    }

    #[test]
    fn entries_that_differ_only_in_where_a_field_ends_hash_differently() {
        let dir = tempfile::tempdir().expect("dir");
        let mut ledger = Ledger::open(&dir.path().join("l.jsonl"), None).expect("open");
        let a = ledger
            .append(Record {
                command_id: "ab".into(),
                proposal_id: Some("c".into()),
                ..record(Kind::Proposed, "x")
            })
            .expect("a");
        let dir2 = tempfile::tempdir().expect("dir");
        let mut ledger2 = Ledger::open(&dir2.path().join("l.jsonl"), None).expect("open");
        let mut b = ledger2
            .append(Record {
                command_id: "a".into(),
                proposal_id: Some("bc".into()),
                ..record(Kind::Proposed, "x")
            })
            .expect("b");
        // Same bytes if fields were simply concatenated; the timestamp is the only other difference, so align it.
        b.ts_ms = a.ts_ms;
        assert_ne!(entry_hash(None, &a), entry_hash(None, &b));
    }

    #[test]
    fn the_hash_ignores_the_order_of_keys_in_the_detail() {
        let base = Entry {
            seq: 0,
            ts_ms: 1,
            kind: Kind::Proposed,
            command_id: "c".into(),
            proposal_id: None,
            binary_sha256: None,
            detail: json!({"a": 1, "b": 2}),
            prev: GENESIS.into(),
            hash: String::new(),
        };
        let reordered = Entry {
            detail: serde_json::from_str(r#"{"b":2,"a":1}"#).expect("json"),
            ..base.clone()
        };
        assert_eq!(entry_hash(None, &base), entry_hash(None, &reordered));
    }

    #[test]
    fn a_second_writer_is_locked_out() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("l.jsonl");
        let _first = Ledger::open(&path, None).expect("first");
        assert!(matches!(
            Ledger::open(&path, None),
            Err(LedgerError::Locked)
        ));
    }

    #[test]
    fn started_without_finished_is_reported_as_unfinished() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("l.jsonl");
        let mut ledger = Ledger::open(&path, None).expect("open");
        ledger.append(record(Kind::Started, "run-1")).expect("a");
        ledger.append(record(Kind::Started, "run-2")).expect("b");
        ledger.append(record(Kind::Finished, "run-1")).expect("c");
        assert_eq!(verify(&path, None).unfinished, vec!["run-2".to_owned()]);
    }

    #[test]
    fn a_short_key_is_refused_and_never_printed() {
        assert!(matches!(
            LedgerKey::new(b"short"),
            Err(LedgerError::WeakKey)
        ));
        assert_eq!(format!("{:?}", key()), "LedgerKey(..)");
    }
}
