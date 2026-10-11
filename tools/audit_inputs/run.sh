#!/usr/bin/env bash
# Text-field audit: runs examples in a real window (needs a display), presses
# every text field with a synthetic pointer at several zoom levels, types into
# it, and reports focus, cursor and selection problems.
#
#   tools/audit_inputs/run.sh [example...]   # default: every example
#   AUDIT_SHOTS=1 tools/audit_inputs/run.sh  # screenshot every probe, not just failures
#
# The examples are patched in a copy under target/, never in place.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
W=$ROOT/target/audit_inputs/work
OUT=${AUDIT_OUT:-$ROOT/target/audit_inputs/out}
mkdir -p "$W" "$OUT"
rsync -a --delete --exclude target --exclude .git "$ROOT/" "$W/"
mkdir -p "$W/examples/audit" && cp "$ROOT/tools/audit_inputs/audit.rs" "$W/examples/audit/mod.rs"
names=("$@")
if [ ${#names[@]} -eq 0 ]; then
  for example in "$W"/examples/*.rs; do
    name=${example##*/}
    names+=("${name%.rs}")
  done
fi
build_args=()
for n in "${names[@]}"; do
  sed -i '0,/App::new()/s//audit::app()/' "$W/examples/$n.rs"
  printf '\nmod audit;\n' >> "$W/examples/$n.rs"
  build_args+=(--example "$n")
done
cd "$W"
export CARGO_TARGET_DIR=$ROOT/target
cargo build -q --locked --all-features "${build_args[@]}"
status=0
for n in "${names[@]}"; do
  rm -f "$OUT/$n.log"
  if ! AUDIT_NAME=$n AUDIT_OUT=$OUT timeout 120 "$CARGO_TARGET_DIR/debug/examples/$n" >"$OUT/$n.stdout.log" 2>"$OUT/$n.stderr.log"; then
    echo "$n: crashed or timed out (see $OUT/$n.stderr.log)"
    status=1
  fi
  if [ -f "$OUT/$n.log" ]; then
    cat "$OUT/$n.log"
  fi
  if ! grep -Eq "^$n: [0-9]+ probes, 0 failed$" "$OUT/$n.log" 2>/dev/null; then
    echo "$n: failed or missing audit summary"
    status=1
  fi
done
echo "Logs and screenshots: $OUT"
exit $status
