# Agent contract

Tool-neutral: what any engineer or coding agent needs to work here without asking.

## Commands

| Task                                                             | Command                                                                              |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Format, clippy (warnings are errors), every test                 | `make check`                                                                         |
| Tests of one crate                                               | `cargo test -p leash-broker` (or `leash-censor`, `-gateway`, `-memory`, `-bridge`, `-server`) |
| Run the server with the lab on                                   | `make up` (then http://localhost:8090); `make down` to stop                          |
| Twelve checks against the running server                         | `make smoke`                                                                         |
| Browser tests and accessibility scans                            | `make e2e` (Playwright in a container, against the real container)                   |
| Break each defence on purpose and confirm a test notices         | `make sabotage` (rewrites `crates/` while it runs: do not edit or build there meanwhile) |
| Re-record the page's sample output                               | `make transcripts` (needs the server running with the lab on)                        |
| Documentation guards                                             | `docsguard check`                                                                    |

## Definition of done

1. A test covers it and fails without the change; a defence gets an adversarial test (try to break it) and, if it is one of
   the critical ones, a mutant in `scripts/sabotage.sh`.
2. `make check` passes and you can quote its output; anything that touches a file, a process or a socket runs against the real thing.
3. An ADR exists for any decision a newcomer would have to reverse-engineer, with the rejected alternatives.
4. `docs/HANDOFF.md` carries the numbers that were printed, not the ones that were expected.

## Boundaries

- Do not weaken the sealing type, the residency rule, the approval digest, the ledger checks or the role check to make a test pass.
- Ask before adding a dependency or changing a route, an error code or an environment variable.
- Anything outside the task goes into `docs/CHECKLIST.md`, not into this change.

## Reporting

Say what happened, not what was expected. A failed test, a skipped check or an unverified claim goes in the report as it is.
