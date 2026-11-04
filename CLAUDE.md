# leash-local-ai

A Rust toolkit for the part of a local AI assistant that has to be right: input the model can only read sealed, private
text that never leaves the machine, commands that run only after a person approves exactly what they were shown, and a
ledger that notices when it is edited. Rust 2024, tokio, axum, rustls; no database, no GPU, no API keys.

## Start here

- Map of every document: [docs/00-READ-FIRST.md](docs/00-READ-FIRST.md). Open a document only when the map says to.
- Resuming work: `docs/HANDOFF.md` first; rewrite it before you stop.
- Commands and definition of done: @AGENTS.md

## Layout

- `crates/leash-censor`: sealing and the instruction screen. `leash-gateway`: providers, residency, circuit breakers.
  `leash-memory`: path guard, chunker, lexical embedder, vector store. `leash-broker`: manifests, pinned binary, approvals,
  ledger. `leash-bridge`: framed protocol, agent state machine, demo PKI, mutual TLS. `leash-server`: axum, the API and the lab.
- `site/`: landing page and lab UI, plain JS and CSS served by the server. `e2e/`: Playwright tests. `corpus/`: the demo documents.
- `scripts/record-transcripts.mjs` writes `site/transcripts.js` from the running server; `scripts/sabotage.sh` breaks each defence on purpose.

## Rules that apply to every task

1. **A provider accepts only a `SafeEnvelope`, and only `seal` makes one.** No public constructor, no `From<String>`. See ADR 0001.
2. **Private text never reaches a remote provider**, whatever the others are doing. No `allow_remote` flag. See ADR 0002.
3. **A command is a manifest, never a command line.** The approval is bound to a digest, claimed once, and the program that runs
   is the one that was hashed (`/proc/self/fd`). See ADR 0003.
4. **The ledger is written before the action it describes**, and verification checks the sequence, the link and the hash. Do not
   remove a check because another one "also catches it": one of them is the only one that catches a pasted entry. See ADR 0004.
5. **The bridge is dormant until an authenticated `activate`, checks the role, and has no operation that takes a command line.** See ADR 0005.
6. **Below the relevance floor the answer is `dont_know` and no model is asked.** See ADR 0006.
7. **The lab never reaches production, and the API refuses a foreign `Host` or `Origin`.** See ADR 0007.
8. `unsafe_code` is denied; the only exception is the `pre_exec` hook in `exec.rs`. No `unwrap`, `expect` or `panic` outside tests.
9. **Tests build their own secrets at run time** and use fictitious data only.
10. **The site loads nothing from another origin** (strict CSP). Do not edit `site/transcripts.js` by hand: `make transcripts`.
11. **Docs are code.** A new document is added to the map in the same commit; `docsguard check` must pass.

Advice versus enforcement: this file is advice. If `docsguard` is installed, `.claude/settings.json` turns the key rules into
hooks (destructive git is refused, and so are writes to secrets and keys).
