# ADR 0007: A lab without a login, that cannot reach production and cannot be driven by a web page

Status: Accepted

## Context
The point of the page is that a visitor can attack the defences: take a model down, swap a program on disk, forge a copy of
the ledger. That needs endpoints that do things no production API should allow, and a visitor should not have to create an
account to try them. A local tool with no login is also exactly what a hostile web page tries to drive from the visitor's
own browser.

## Decision
- **The lab is off unless asked for.** Without `LEASH_LAB` every `/api/lab/*` endpoint answers 404 `lab_disabled`, as if it
  did not exist.
- **The lab cannot start in production.** `LEASH_LAB=1` together with `LEASH_ENV=production` is a configuration error and the
  server refuses to start. A test pins it. A mutant that removes the check is killed.
- **Every lab action is on a copy or in the demo's own data.** The ledger forgeries run on a copy of the file. The program swap
  changes a binary that the server copied into its data directory for that purpose. The impostors connect to a fresh agent.
  Nothing outside the demo's data directory is touched.
- **The API refuses to be driven by a web page.** The `Host` header must be one of the server's own names (`localhost`,
  `127.0.0.1`, `[::1]`, plus any the operator lists) and an `Origin`, if present, must match. A page on another site that
  resolves to `127.0.0.1` still sends its own name in `Host`, so DNS rebinding fails; a cross-site request carries its own
  `Origin`, so it fails too.
- **The container is published on the loopback only** (`127.0.0.1:8090`), is read-only, drops every capability and runs as
  a non-root user.
- **A strict content security policy** (`default-src 'self'`, no inline script, no third-party request) and a request body
  limit of 64 KiB.
- **No login, on purpose, and said so.** The page's "Enter the lab" button is the whole sign-in.

## Rejected alternatives
- **A login for the lab.** A demo account is a password to keep and to leak, and a barrier between a recruiter and the thing
  they came to see.
- **A flag in the request that enables lab behaviour.** The mode belongs to the process, not to the caller.
- **Rely on the network.** "Only reachable locally" is true until it is published for a demo.
- **Trust `Origin` alone.** It is absent on many same-origin requests and on non-browser clients; `Host` is the one that
  defeats rebinding.

## Consequences
- (+) Three mutants guard it (`scripts/sabotage.sh`): any `Host` served, any `Origin` served, and the lab allowed in
  production. All killed.
- (+) The smoke test checks a foreign `Host` and a foreign `Origin` against the running container.
- (−) **Anyone who can reach the port can use the lab.** With the default publishing that is the local machine. Binding to
  a public interface with the lab on would be a mistake, and the server cannot detect it.
- (−) The lab's *Start over* cannot reset the append-only ledger ([ADR 0004](0004-a-signed-ledger-and-its-limits.md)); it
  resets everything else.
