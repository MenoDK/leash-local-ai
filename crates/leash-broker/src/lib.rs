//! # leash-broker
//!
//! The only way anything is executed. A person **binds** a command: a manifest naming one single-purpose program, its
//! SHA-256, the arguments it takes (typed) and the flags it may be given. The assistant can only *propose* a bound
//! command with arguments; the broker validates them, shows a digest of exactly what would run, waits for a person to
//! approve that digest, runs it once without a shell, and records every step in a hash-chained ledger.
//!
//! What this protects against, in order of where it bites:
//!
//! 1. A prompt-injected model asking for something it should not: it can only name bound commands, with typed arguments.
//! 2. Argument injection: no leading `-`, a `--` separator, flags matched byte for byte, no shell to interpret anything.
//! 3. A swapped binary: the hash is checked on the descriptor that is executed.
//! 4. A confused approval: approving a digest approves exactly that vector, once.
//! 5. A rewritten history: the ledger is a hash chain (optionally keyed).
//!
//! What it does not do is sandbox the program: see `exec`.

mod broker;
mod exec;
mod ledger;
mod manifest;
mod validate;

pub use broker::{
    Actor, Broker, BrokerConfig, Decision, Proposal, Refusal, Resolution, ResolveError, State,
};
pub use exec::{ExecError, Execution, PinError, PinnedBinary, run as run_pinned, sha256_of};
pub use ledger::{
    AnchorMismatch, Break, Entry as LedgerEntry, GENESIS, Kind as LedgerKind, Ledger, LedgerError,
    LedgerKey, Record as LedgerRecord, Verification, check_anchor, verify as verify_ledger,
};
pub use manifest::{Approval, ArgKind, ArgSpec, Limits, Manifest, ManifestError, load_dir};
pub use validate::{ValidateError, build_argv};
