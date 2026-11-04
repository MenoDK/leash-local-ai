# ADR 0001: The model only reads sealed input, and the type system is what enforces it

Status: Accepted

## Context
Everything an assistant reads can be written by someone else: a tool's output, a document in a folder, a web page, an e-mail.
If that text can be mistaken for an instruction, then anyone who can put text in front of the assistant can steer it.
"Be careful to clean the text before the prompt" is a convention, and a convention is broken by the first new code path
that forgets it.

## Decision
- **One way in.** The only type a provider accepts is `SafeEnvelope`. Its fields are private and it has no public
  constructor; `leash_censor::seal` is the only function that returns one. A function that wants to talk to a model cannot
  be written without going through it, and a `compile_fail` doctest keeps that true: building an envelope by hand must
  fail to compile.
- **What sealing does**, in this order: bound the input; NFKC-normalise (so full-width letters and look-alikes collapse);
  remove terminal escape sequences, control characters and invisible formatting characters; redact secrets; neutralise
  chat-template tokens; replace `<` and `>` with look-alike angle quotes so the text cannot close its own wrapper; cut to
  a character limit. The result is JSON-encoded inside `<untrusted_data provenance="…">`.
- **Provenance travels with the text.** `ToolOutput`, `Document` or `Confidential`. The last one also decides where the
  text may be processed ([ADR 0002](0002-private-text-fails-closed.md)).
- **A screen, for the cases where the text addresses the model.** A set of patterns (a chat-template token, a role marker,
  an "ignore the previous instructions" family in English, Portuguese and Spanish, an identity reset, an exfiltration
  request). Untrusted text that matches is *held for a person*: the server answers 422 `held_for_review` and no provider
  is called. In the memory, a passage that matches is kept out of the index.
- **The screen is not the defence.** It is a list of patterns, and the lab prints a paraphrase that walks past it. What
  holds when it fails is that nothing a model says can run on its own ([ADR 0003](0003-a-broker-instead-of-a-shell.md)).

## Rejected alternatives
- **A function `clean(&str) -> String` called "before the prompt".** The next caller forgets it. Nothing fails.
- **Asking the model to ignore instructions inside the data.** A request made in the same channel as the attack.
- **A classifier model as the gate.** A model deciding whether text is safe for a model is the thing being protected, now
  with a second target. Useful as a signal, wrong as a boundary.
- **Escaping only the characters we thought of.** The envelope is JSON-encoded as a whole, so the text is a string value,
  not markup, whatever it contains.
- **Dropping the screen because it can be bypassed.** It is cheap, it stops the careless attack, and it gives a person a
  reason to look. It is labelled for what it is.

## Consequences
- (+) Every defence above has a test, and two have a mutant that removes them (`scripts/sabotage.sh`): the angle quotes and
  the invisible-character filter. A third removes the screen.
- (−) A paraphrase gets through the screen. Said on the page, shown in the lab, and the reason the broker exists.
- (−) NFKC changes the text (a ligature becomes two letters). Acceptable for an assistant; wrong for a tool that must return
  bytes unchanged.
- (−) The secret redaction is pattern-based too: a key in an unusual shape is not recognised.
