# ADR 0003: A broker instead of a shell: manifests, a pinned binary, and an approval bound to a digest

Status: Accepted

## Context
The point of an assistant on a real machine is that it can do things. The simplest way to let it is a tool that takes a
command line. Then a model that has been fooled, once, runs whatever the fooler wanted, and a deny list names only the
things somebody thought of.

## Decision
- **No shell, no command line.** A command is a *manifest*: an id, one program identified by path **and SHA-256**, typed
  arguments (`Token` or `Enum`), a fixed list of allowed flags, resource limits and a timeout. The model chooses an id and
  fills in arguments; it cannot say anything else.
- **Arguments cannot become options.** A value that starts with `-` is refused, a token is a short run of safe characters
  (so `a; id` is not one), and positional arguments are placed after `--` unless the manifest says the program does not
  take it.
- **The program that runs is the program that was hashed.** The binary is opened with `O_NOFOLLOW`, hashed through that
  same descriptor, and executed as `/proc/self/fd/N`. Swapping the file for a different one a microsecond later changes
  nothing. The hash is checked again after approval, not only when the proposal is made.
- **A proposal waits for a person.** The digest shown to the approver covers the command, the program's hash and every
  argument. Approving a different digest is refused; approving the right one claims the proposal under a lock, so an
  approval is **used once** and two concurrent approvals start exactly one process.
- **Who may approve is part of the manifest.** `Always` needs a human; `Never` (read-only, harmless commands) the policy
  may approve itself. A policy cannot approve what needs a person, and a test says so.
- **A clean room for the child.** Empty environment, no stdin, `no_new_privs`, CPU / memory / file-size / descriptor /
  core limits, its own process group killed as a whole on timeout, output capped. Its output is sealed
  ([ADR 0001](0001-the-model-only-reads-sealed-input.md)) before anyone, or any model, reads it.
- **The queue of waiting proposals is bounded**, so a loop of proposals cannot grow memory.
- **Everything is written to the ledger before it happens** ([ADR 0004](0004-a-signed-ledger-and-its-limits.md)).

## Rejected alternatives
- **A shell tool with a deny list.** Names what you thought of. Quoting, globbing, `$(…)`, an interpreter reached through
  an allowed program: the list is never finished.
- **An allow-list of command strings matched by regular expression.** The regular expression becomes the parser, and the
  parser is the attack surface.
- **Approve by path.** The path you approved is not necessarily the program that runs.
- **A container per command.** The right extra layer for production and a different project; it is listed under "Not built
  yet".
- **`sudo` rules.** Solves privilege, not intent.

## Consequences
- (+) Four mutants guard it (`scripts/sabotage.sh`): a flag-shaped argument accepted, any digest approving, an approval
  usable twice, and a swapped program still running. All killed. (A fifth, on the ledger's link check, is in
  [ADR 0004](0004-a-signed-ledger-and-its-limits.md).)
- (+) The lab has a button that approves a proposal like a person and then changes the program by one byte; the run is
  refused.
- (−) **It is not a sandbox.** An approved program runs with the server's rights, limited by rlimits and `no_new_privs`, with
  no namespaces, no seccomp and no Landlock. Approving a program that is dangerous on its own is still your decision.
- (−) The one `unsafe` block in the workspace is the `pre_exec` hook that sets the limits (it is unsafe by signature; the
  closure calls only async-signal-safe functions). `unsafe_code` is denied everywhere else.
- (−) Linux only: it needs `/proc/self/fd`. The crate refuses to compile elsewhere rather than run with a weaker check.
