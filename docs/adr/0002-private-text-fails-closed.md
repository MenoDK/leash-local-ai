# ADR 0002: Private text fails closed, and there is no fallback for it

Status: Accepted

## Context
A local model goes down: out of memory, a crashed process, a laptop that just woke up. The usual answer is "try the next
provider", and the next one is a hosted model. For a public sentence that is resilience. For the owner's private notes it
is a leak that looks like a feature, and it happens exactly when nobody is watching.

## Decision
- **Residency is a property of the text.** Sealing a `Confidential` input yields `Residency::LocalOnly`. The gateway reads
  it from the envelope; the caller cannot forget to pass it.
- **A remote provider is never called for local-only text**, whatever the state of the others. The trace says so
  (`skipped_remote`) so that "it did not leave" is something you can read, not something you are told.
- **"Local" is derived from the address, not from a setting.** The Ollama provider accepts only a loopback URL and
  reports `Locality::Local` from that; a hosted provider reports `Remote`. There is no `allow_remote` flag to turn on at
  two in the morning.
- **No eligible provider means an honest refusal**: `every eligible provider failed or is cooling down`, with the trace.
- **A circuit breaker per provider**, so that a provider that keeps failing is not asked again for a while: three
  consecutive failures open it, after twenty seconds exactly one probe is allowed through, and a failed probe reopens it
  for a full cooldown. This protects the provider and the caller's latency; it is *not* what protects the data.
- **Calls have a time limit.** A provider that hangs is a failure, not a wait.

## Rejected alternatives
- **Fall back to a hosted model after redacting the text.** Redaction is pattern-based ([ADR 0001](0001-the-model-only-reads-sealed-input.md));
  the failure case is the one nobody tested.
- **A per-request `allow_remote` parameter.** Moves the decision to every call site, which is where it is forgotten.
- **Queue the request until the local model is back.** Reasonable for batch work; for an assistant the owner is waiting on,
  an honest "not now" is better than silence.
- **Retry forever with backoff.** Hides the outage and keeps the model busy; the breaker makes it visible.

## Consequences
- (+) Two mutants guard it (`scripts/sabotage.sh`): one lets private text reach a remote provider, one stops the circuit
  from ever opening. Both are killed.
- (+) The lab lets you take the local model down and send a private sentence, then read the trace.
- (−) When the local model is down, private questions get no answer. That is the cost, chosen on purpose.
- (−) The two models in the demo are deterministic stand-ins that do not generate text. The routing, the breaker and the
  trace are real; the language is not. Point the server at a local Ollama to use a real model.
