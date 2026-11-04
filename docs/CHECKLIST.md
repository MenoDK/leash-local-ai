# Master checklist

_What_ and _when_. The _how_ lives in the code and tests, the _why_ in the [ADRs](adr/README.md).

| #   | Item                                                                                  | Status  | Evidence                                                                                  |
| --- | ------------------------------------------------------------------------------------- | ------- | ----------------------------------------------------------------------------------------- |
| 1   | Sealing: one way to build the type a provider accepts; the instruction screen         | done    | [ADR 0001](adr/0001-the-model-only-reads-sealed-input.md), `crates/leash-censor`           |
| 2   | Gateway: residency, per-provider circuit breakers, a trace of what was skipped        | done    | [ADR 0002](adr/0002-private-text-fails-closed.md), `crates/leash-gateway/tests/routing.rs` |
| 3   | Broker: manifests, pinned binary, digest-bound single-use approval, clean-room child  | done    | [ADR 0003](adr/0003-a-broker-instead-of-a-shell.md), `crates/leash-broker/tests/`          |
| 4   | Ledger: keyed chain, torn-line detection, anchors, splice test                        | done    | [ADR 0004](adr/0004-a-signed-ledger-and-its-limits.md), `crates/leash-broker/src/ledger.rs` |
| 5   | Bridge: dormant agent, role check, mutual TLS 1.3, five impostors                     | done    | [ADR 0005](adr/0005-a-bridge-that-sleeps.md), `crates/leash-bridge`                        |
| 6   | Memory: path guard, redaction, quarantine, relevance floor                            | done    | [ADR 0006](adr/0006-memory-that-says-i-dont-know.md), `crates/leash-memory`                |
| 7   | Server: API, lab, Host/Origin checks, refused in production                           | done    | [ADR 0007](adr/0007-a-lab-without-a-login.md), `crates/leash-server/tests/api.rs`          |
| 8   | Landing page and lab in 14 languages; recorded tour with a drift check                | done    | `site/`, `scripts/record-transcripts.mjs`                                                  |
| 9   | Mutation check of the defences                                                        | done    | `scripts/sabotage.sh`, `make sabotage`                                                     |
| 10  | Container, health-gated compose, smoke test                                           | done    | `Dockerfile`, `docker-compose.yml`, `scripts/smoke.sh`                                     |
| 11  | Browser tests with accessibility scans                                                | done    | `e2e/`, `make e2e`                                                                         |
| 12  | CI workflow                                                                           | done    | `.github/workflows/ci.yml`: all six jobs green on the first run (2026-11-04), `cargo audit` included |
| 13  | Sandbox around the child (namespaces, seccomp, Landlock)                              | todo    | ADR 0003                                                                                   |
| 14  | Open memory files descriptor-first (`openat`, `O_NOFOLLOW`) to close the walk/read window | todo | ADR 0006                                                                                   |
| 15  | Receipts in write-once storage; a key id on each ledger line for rotation             | todo    | ADR 0004                                                                                   |
| 16  | A neural embedder behind the `Embedder` trait                                         | todo    | ADR 0006: `OllamaEmbedder` written, not wired into the server, never run on a real model   |
| 17  | The vsock transport run between a real host and a real guest                          | todo    | ADR 0005                                                                                   |
| 18  | The Ollama provider tested against a real model                                       | todo    | README "Limitations"                                                                       |

Status is one of `todo`, `doing`, `done`, `written` (exists, not yet exercised where it will run), `dropped`.
