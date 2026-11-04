#!/usr/bin/env bash
# Mutation check by hand: break one defence at a time and require the tests of that crate to go RED.
# A suite that stays green on a broken defence proves nothing, and this is the proof that each defence is watched.
#
#   scripts/sabotage.sh            all mutants
#   scripts/sabotage.sh seal       only the mutants whose name contains "seal"
#
# It builds in its own target directory (target/sabotage), so a server you have running from target/debug is not
# replaced by a broken binary. The source tree is restored after every mutant, and checked byte for byte at the end.
set -uo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target/sabotage}"
only="${1:-}"

tree_hash() { find crates -type f -name '*.rs' -print0 | sort -z | xargs -0 sha256sum | sha256sum; }
before=$(tree_hash)
survivors=0
errors=0
total=0
current_file=""
current_backup=""

restore() {
  if [ -n "$current_backup" ] && [ -f "$current_backup" ]; then cp -p "$current_backup" "$current_file"; rm -f "$current_backup"; fi
  current_backup=""
}
trap restore EXIT INT TERM

# usage: mutant "<name>" <file> "<old>" "<new>" <package>
mutant() {
  local name="$1" file="$2" old="$3" new="$4" package="$5"
  if [ -n "$only" ] && [[ "$name" != *"$only"* ]]; then return; fi
  total=$((total + 1))
  current_file="$file"
  current_backup=$(mktemp)
  cp -p "$file" "$current_backup"
  if ! python3 - "$file" "$old" "$new" <<'PY'
import pathlib, sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
text = pathlib.Path(path).read_text()
if text.count(old) != 1:
    sys.exit(f"pattern must occur exactly once in {path}, found {text.count(old)}")
pathlib.Path(path).write_text(text.replace(old, new))
PY
  then
    echo "  [setup error] $name"
    restore
    errors=$((errors + 1))
    return
  fi
  local out
  out=$(timeout 900 cargo test -p "$package" 2>&1)
  restore
  # "error: test failed" is cargo's normal report of a red test; only "could not compile" means the mutant never built.
  if grep -q "could not compile" <<<"$out"; then
    echo "  [does not compile] $name   (a mutant that does not build proves nothing)"
    grep -E -B8 "could not compile" <<<"$out" | head -14 | sed 's/^/      /'
    errors=$((errors + 1))
  elif grep -qE "^test result: FAILED|panicked at" <<<"$out"; then
    echo "  killed    $name   ($(grep -cE '^test .* FAILED$' <<<"$out") test(s) failed in $package)"
  else
    echo "  SURVIVED  $name   (every test of $package still passed)"
    survivors=$((survivors + 1))
  fi
}

echo "mutants (each must be killed):"
mutant "seal: angle brackets no longer neutralised"      crates/leash-censor/src/lib.rs        "'<' => '‹'," "'<' => '<',"                                                              leash-censor
mutant "seal: invisible characters survive"              crates/leash-censor/src/text.rs       "if is_invisible(c) {" "if false && is_invisible(c) {"                                    leash-censor
mutant "screen: instruction rules never fire"            crates/leash-censor/src/quarantine.rs ".find(|rule| rule.pattern.is_match(&canonical))" ".find(|rule| rule.pattern.is_match(&canonical) && false)" leash-censor
mutant "route: private text may reach a remote model"    crates/leash-gateway/src/gateway.rs   "if local_only && locality == Locality::Remote {" "if false && locality == Locality::Remote {"    leash-gateway
mutant "route: the circuit never opens"                  crates/leash-gateway/src/breaker.rs   "|| inner.consecutive_failures >= self.threshold" "|| false"                                   leash-gateway
mutant "memory: a path may climb with .."                crates/leash-memory/src/guard.rs      "Component::ParentDir => return Err(GuardError::Traversal)," "Component::ParentDir => {}"                leash-memory
mutant "memory: a link is followed"                      crates/leash-memory/src/guard.rs      "Ok(meta) if meta.file_type().is_symlink() => return Err(GuardError::Symlink)," "Ok(meta) if false && meta.file_type().is_symlink() => return Err(GuardError::Symlink)," leash-memory
mutant "memory: no relevance floor"                      crates/leash-memory/src/memory.rs     "floor: 0.30," "floor: 0.0,"                                                              leash-memory
mutant "broker: a flag-shaped argument is accepted"      crates/leash-broker/src/validate.rs   "if value.starts_with('-') {" "if false && value.starts_with('-') {"                      leash-broker
mutant "broker: any digest approves"                     crates/leash-broker/src/broker.rs     "if digest != entry.proposal.digest {" "if false {"                                      leash-broker
mutant "broker: an approval can be used twice"           crates/leash-broker/src/broker.rs     "if entry.proposal.state != State::Pending {" "if false {"                              leash-broker
mutant "broker: a swapped program still runs"            crates/leash-broker/src/exec.rs       "if !actual.eq_ignore_ascii_case(expected_sha256.trim()) {" "if false {"                  leash-broker
mutant "ledger: the link to the previous entry is not checked" crates/leash-broker/src/ledger.rs "if entry.prev != report.head {" "if false {"                                           leash-broker
mutant "bridge: the caller's role is not checked"        crates/leash-bridge/src/session.rs    "let response = if peer_is_controller {" "let response = if true {"                        leash-bridge
mutant "server: any Host header is served"               crates/leash-server/src/security.rs   ".is_some_and(|value| allowed.contains(&host_name(value)));" ".is_some_and(|_| true);" leash-server
mutant "server: any Origin is served"                    crates/leash-server/src/security.rs   ".is_some_and(|host| allowed.contains(&host));" ".is_some_and(|_| true);"           leash-server
mutant "server: the lab is allowed in production"        crates/leash-server/src/config.rs     "if lab && production {" "if false && production {"                                      leash-server

after=$(tree_hash)
if [ "$before" = "$after" ]; then echo "source tree restored byte for byte"; else echo "SOURCE TREE CHANGED, restore it manually"; exit 2; fi
echo "mutants run: $total   survivors: $survivors   setup or build errors: $errors"
[ "$survivors" -eq 0 ] && [ "$errors" -eq 0 ]
