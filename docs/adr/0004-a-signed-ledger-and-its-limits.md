# ADR 0004: A keyed ledger, and a plain statement of what it cannot prove

Status: Accepted

## Context
After something goes wrong, the first question is what the assistant was asked to do and what it did, and the second is
whether the record that answers the first can be trusted. A log file is the first thing edited by whoever wants the story
to change.

## Decision
- **An append-only JSON-lines file.** Every request, refusal, approval, start and finish is one line with a sequence
  number and the hash of the line before it.
- **The hash is an HMAC-SHA256 under a key** (`LEASH_LEDGER_KEY`), so that changing a line and recomputing the hashes is
  not possible without the key. Without a key the chain is still a chain, but anyone can rebuild it, and the server says so
  when it starts.
- **Verification checks, in this order, and names what failed**: the line is complete (a torn last line is reported, and
  the ledger refuses to reopen over it), the sequence number is the next one, the line points at the previous hash, and the
  hash matches the contents.
- **A single writer.** The file is locked (`flock`) for the life of the process; a second process cannot open it.
- **`Started` and `Finished` are separate records**, so a crash between them is visible as an *unfinished* command instead
  of silence.
- **The ledger is written before the thing it describes happens.** If the write fails, the command does not run.
- **An anchor is a pair `(length, head)`** kept somewhere the writer cannot reach. `check_anchor` compares a ledger with
  it and says *truncated* or *rewritten*.

## What it proves, and what it does not
| Someone who can… | Is… |
| --- | --- |
| edit a line | caught: the hash no longer matches |
| remove or reorder lines | caught: the sequence number or the link breaks |
| paste a line signed in another history under the same key | caught: it does not follow the previous line |
| rebuild the chain **without** the key | caught |
| **cut the last lines off** | **not caught by the chain**: a shorter chain is still a valid chain. Caught only by an anchor taken earlier |
| rebuild the chain **with** the key | not caught by the chain; caught by an anchor |
| rebuild the chain, hold the key **and** reach the anchors | not caught. A witness the attacker cannot write to is the whole requirement |
| rebuild an **unkeyed** chain | not caught; this is what the key is for |

The lab has a button for each of the four forgeries (`edit`, `reorder`, `rewrite`, `truncate`) and runs them on a **copy**,
never on the real ledger. It prints who caught each one: the chain, or only the anchor.

## Rejected alternatives
- **A plain hash chain.** Recomputable by anyone.
- **Hash the line only, not the previous hash.** Lines could be removed or reordered undetected.
- **A database.** A bigger system with its own admin, to protect a file that fits on a screen. The format is the design:
  `cat`, `grep` and `jq` are the audit tools.
- **A public transparency log.** Strong, and much more than a demo needs. The anchor format would not change.
- **Rotate the key on every record.** Hides the problem instead of solving it; a ratchet is a real option and not built.

## Consequences
- (+) Each row of the table has a test that causes the damage, and `scripts/sabotage.sh` removes the link check. **That
  mutant survived the first version of the tests.** The sequence check and the hash check each catch an edit, a deletion and
  a reorder, so every test passed without the link check; only the pasted-from-another-history case needs it. The test
  `an_entry_signed_in_another_history_cannot_be_spliced_in` was written for that, and the mutant is now killed.
- (−) Appends are serialised and each one is flushed; the throughput is one writer at a time. Fine for decisions, wrong for a
  request log.
- (−) The key is one point of trust, and there is no key id on each line, so rotation is not built.
- (−) The lab's *Start over* cannot reset the ledger, because it is append-only on purpose.
