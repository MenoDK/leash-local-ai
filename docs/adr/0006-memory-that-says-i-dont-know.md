# ADR 0006: A memory that says "I don't know", and refuses to read what it should not

Status: Accepted

## Context
An assistant that answers from a folder of the owner's documents has two ways to fail that matter. It can answer when the
documents have nothing relevant, and the model fills the gap with something plausible. And it can read, or hand back, things
that were never meant to be in the folder's index: a symlink to `/etc/passwd`, a file called `passwords-list`, a note where
somebody pasted their environment, a document that talks to the model.

## Decision
- **A path guard decides what exists to the memory.** A file is indexed or read only if it is a regular file, below a `docs`
  directory, with a known extension (`md`, `txt`), under a size limit, with a name that does not suggest a credential
  (`secret`, `password`, `token`, `id_rsa`, `.pem`, `.key` …), and not under a hidden or build directory. **It never follows a
  link**: every component of the path is inspected without following, so a link to a file outside the root, or a directory
  link, is refused. `..` is refused outright. The same guard decides what can be *read by path*, so there is one rule, not two.
- **Secrets are redacted when a document is indexed**, and the report says how many. A note with a pasted environment is
  indexed with its values replaced; what the model is given never contained them.
- **A passage that addresses the model is held out of the index** (the screen from [ADR 0001](0001-the-model-only-reads-sealed-input.md)),
  and the index report names the document, the passage and the rule.
- **Retrieval has a floor.** The best passages above a relevance score of 0.30 (at most three) are returned. If none is
  above it, the answer is `dont_know`, the nearest score is shown, and **no model is asked**. That is a decision made before
  the model, so the model has no chance to improvise.
- **An exact vector store**, not an approximate one: a few hundred passages need no index structure, and an exact search has
  no recall to explain.
- **Retrieved text is sealed like everything else** before it reaches a provider, with `Document` provenance.

## Rejected alternatives
- **Let the model say "I don't know".** It will, sometimes. The floor makes the cheap case certain and leaves the model the
  hard one.
- **Check the path with a string prefix.** `docs/../../etc/passwd` and a link both pass a prefix test.
- **Canonicalise the path and compare it with the root.** That follows the links first and judges afterwards. The guard
  looks at each component without following it and refuses the link itself.
- **A hosted embedding model.** Sends the owner's documents somewhere to be indexed ([ADR 0002](0002-private-text-fails-closed.md)).
- **Approximate nearest neighbours.** Needed at a scale this is not.

## Consequences
- (+) Three mutants guard it (`scripts/sabotage.sh`): a `..` allowed, a link followed, and no relevance floor. All killed.
- (+) The lab hands the guard a traversal, a link and a credential-named file and prints each refusal by name.
- (−) **The embedder is lexical** (a 512-dimensional hashed bag of words). It finds the passage that shares words with the
  question and cannot find one that only shares meaning. A neural embedder fits behind the same trait: an `OllamaEmbedder`
  adapter (loopback only) is written, but the server does not use it, only its address check is tested, and it has never
  been run against a real model.
- (−) **There is a window between the guard's walk and the read.** `resolve` returns a path and the file is then opened
  by that path, without `O_NOFOLLOW` (the broker opens its binary that way; the memory does not). Someone who can already
  write inside the documents folder could swap a file for a link in that window. Closing it means opening the file
  descriptor-first and walking with `openat`; it is on the list of what is not built.
- (−) The floor is a constant, tuned on the demo documents. A real corpus needs its own.
- (−) The redaction is pattern-based. A secret in an unusual shape is indexed.
