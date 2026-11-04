# Architecture decision records

Each record states the problem, what was chosen, **what was rejected and why**, and the cost that was accepted.

| # | Decision |
| --- | --- |
| [0001](0001-the-model-only-reads-sealed-input.md) | The model only reads sealed input, and the type system is what enforces it |
| [0002](0002-private-text-fails-closed.md) | Private text fails closed, and there is no fallback for it |
| [0003](0003-a-broker-instead-of-a-shell.md) | A broker instead of a shell: manifests, a pinned binary, and an approval bound to a digest |
| [0004](0004-a-signed-ledger-and-its-limits.md) | A keyed ledger, and a plain statement of what it cannot prove |
| [0005](0005-a-bridge-that-sleeps.md) | A bridge that sleeps until the right certificate wakes it, and has no command line |
| [0006](0006-memory-that-says-i-dont-know.md) | A memory that says "I don't know", and refuses to read what it should not |
| [0007](0007-a-lab-without-a-login.md) | A lab without a login, that cannot reach production and cannot be driven by a web page |

A record is never rewritten to hide a reversal: a new record supersedes it and the old one says so
(`Superseded by: ADR NNNN`). Template and rules: the `adr` skill in `.claude/skills/adr/`.
