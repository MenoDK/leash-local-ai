# Handoff

Updated: 2026-11-04

Rewritten at the end of every session. A fresh session must be able to resume from disk alone.

## 1. What is live

Nothing is deployed. `make up` builds the image and runs one container with the site, the lab and the API on
http://localhost:8090 (published on the loopback only). The lab is on in that container; the server refuses to start with the
lab on when `LEASH_ENV=production`.

## 2. Where we stopped

The six crates, the server, the landing page and lab in 14 languages, the browser tests, the smoke test, the mutation check,
the delivery files and the documentation are complete and verified locally (section 8). The CI workflow is written and has not
run on GitHub yet.

## 3. What the owner does now

1. Have native speakers read the thirteen translations in `site/i18n/` (written by the assistant; Hindi, Bengali, Urdu and Arabic most of all).
2. Decide whether to make the repository public.

## 4. Deadlines

None.

## 5. What NOT to do

- Do not add a way to build a `SafeEnvelope` other than `seal`, and do not give a provider an entry point that takes a string: [ADR 0001](adr/0001-the-model-only-reads-sealed-input.md).
- Do not add an `allow_remote` switch, per request or global: [ADR 0002](adr/0002-private-text-fails-closed.md).
- Do not add an operation that takes a command line, to the broker or to the bridge: [ADR 0003](adr/0003-a-broker-instead-of-a-shell.md), [ADR 0005](adr/0005-a-bridge-that-sleeps.md).
- Do not delete a ledger check because another one seems to cover it. The link to the previous entry looked redundant, survived the
  first mutation run, and is the only check that refuses an entry pasted from another history: [ADR 0004](adr/0004-a-signed-ledger-and-its-limits.md).
- Do not edit `site/transcripts.js` by hand. Change the behaviour, then run `make transcripts`; CI runs `--check`.
- Do not run `scripts/sabotage.sh` while editing `crates/`: it rewrites source files in place and restores them.
- Do not use a bare `ls` in this environment's shell (it is aliased and hangs); use `command ls`.
- Do not let the browser container create `docs/img` by itself: Docker makes a missing bind-mount source as root. `make e2e` creates it first.

## 6. Decisions

- Sealing, residency, the broker, the ledger, the bridge, the memory and the lab: [ADR 0001](adr/0001-the-model-only-reads-sealed-input.md) to [ADR 0007](adr/0007-a-lab-without-a-login.md).

## 7. Learned, not in the code

- A test that reopened the ledger right after dropping the broker failed about one run in fifteen with `Locked`. The lock is
  `flock`, which belongs to the open file description, and the tests of that file run on parallel threads of one process; a
  sibling test that forks a child holds a copy of every descriptor between `fork` and `exec`. Run with one thread the failure was
  0 in 150; the test now waits a bounded time for the lock (`reopen` in `tests/broker.rs`) and 300 runs are clean.
- The first mutation run had one survivor: removing the ledger's link check. It was a real gap in the tests, not in the
  system, and `an_entry_signed_in_another_history_cannot_be_spliced_in` now covers it. 17 of 17 mutants are killed.
- Cargo prints `error: test failed, to rerun pass …` when a test is red. The sabotage script first read that as a build error;
  only `could not compile` means the mutant never built.
- The lab shows file reads and questions in two columns. A browser test that takes the first card finds the wrong one; pick the
  card by the text it must contain (`latest()` in `e2e/tests/lab.spec.ts`).
- Re-recording the tour after the server gained the `behaviour` field changed one transcript. The recorder prints only what is
  deterministic (never the ledger's growing counts), and `--check` passes twice in a row.
- The memory opens a file by the path the guard returned, without `O_NOFOLLOW`. That window is real and documented in
  [ADR 0006](adr/0006-memory-that-says-i-dont-know.md) and the checklist.
- `kit.css`, `kit.js` and `i18n/ui.js` (the page chrome in 14 languages) are shared with the other showcase repositories; the
  canonical copy is in the first one and `scripts/sync-kit.sh` copies it. Change a shared style there, not here.

## 8. Suite state

Measured on 2026-11-04: 215 tests (`cargo test --workspace`, doc tests included); `cargo fmt --check` and `cargo clippy
-D warnings` clean, also with the `vsock` feature; 17 of 17 mutants killed (`make sabotage`); 53 browser tests against the
Compose container (`make e2e`: axe scans, 14 languages, the lab), plus 4 screenshot tests skipped unless `SHOTS=1`; the smoke test
(12 steps) passes against the container and again after a restart on the same volume; the recorded tour matches the server
(`--check`, twice); `hadolint` and `actionlint` clean.
