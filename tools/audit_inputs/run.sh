#!/usr/bin/env bash
# Text-field audit: runs examples in a real window (needs a display), presses
# every text field with a synthetic pointer at several zoom levels, types into
# it, and reports focus, cursor and selection problems.
#
#   tools/audit_inputs/run.sh [example...]   # default: every example
#   AUDIT_SHOTS=1 tools/audit_inputs/run.sh  # screenshot every probe, not just failures
#
# The examples are patched in a copy under target/, never in place.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
W=$ROOT/target/audit_inputs/work
OUT=${AUDIT_OUT:-$ROOT/target/audit_inputs/out}
mkdir -p "$W" "$OUT"
rsync -a --delete --exclude target --exclude .git "$ROOT/" "$W/"
mkdir -p "$W/examples/audit" && cp "$ROOT/tools/audit_inputs/audit.rs" "$W/examples/audit/mod.rs"
names=("$@")
[ ${#names[@]} -eq 0 ] && names=($(cd "$W/examples" && ls *.rs | sed 's/\.rs$//'))
for n in "${names[@]}"; do
  sed -i '0,/App::new()/s//audit::app()/' "$W/examples/$n.rs"
  printf '\nmod audit;\n' >> "$W/examples/$n.rs"
done
cd "$W"
export CARGO_TARGET_DIR=$ROOT/target
cargo build -q --all-features $(printf -- '--example %s ' "${names[@]}") || exit 1
status=0
for n in "${names[@]}"; do
  AUDIT_NAME=$n AUDIT_OUT=$OUT timeout 120 "$CARGO_TARGET_DIR/debug/examples/$n" 2>/dev/null | grep -E "^ |: " \
    || { echo "$n: no result (crashed or timed out)"; status=1; }
  grep -q "FAIL" "$OUT/$n.log" 2>/dev/null && status=1
done
echo "Logs and screenshots: $OUT"
exit $status
