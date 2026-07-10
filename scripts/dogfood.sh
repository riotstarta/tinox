#!/usr/bin/env bash
# Dogfood-Gate (TESTPLAN Phase 3): examples/ + benchmarks/ bauen (mit
# Smoke-Runs, wo deterministisch) und jgrep-tinox bauen + Tests fahren.
# Aufruf über `make dogfood`; erwartet ein frisches target/release/tinox.
set -uo pipefail
cd "$(dirname "$0")/.."

TINOX="$PWD/target/release/tinox"
DOGFOOD_DIR="${DOGFOOD_DIR:-../jgrep-tinox}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

FAIL=0
step() { printf '  %-44s' "$1"; }
ok()   { echo "OK"; }
bad()  { echo "FAIL"; FAIL=1; }

echo "== Dogfood: examples bauen =="
# Bekannt kaputte Beispiele (vorbestehend, siehe TESTPLAN Phase 3):
#   examples.tnx        — Int32/Int64-Mix im Codegen (add i32 mit i64-Operand)
#   interface_extends   — Library-Datei ohne main
#   mini_http.tnx       — Library-Modul ohne main
#   rest_with_mini.tnx  — @Json_deserialize nicht implementiert
#   modules/multi_import — veraltete `import a::b;`-Syntax
GOOD_EXAMPLES=(
    examples/cli_test.tnx
    examples/simple_test.tnx
    examples/vtable_dispatch.tnx
    examples/rest_minimal.tnx
    examples/rest_test.tnx
    examples/modules/main.tnx
)
for f in "${GOOD_EXAMPLES[@]}"; do
    step "$f"
    if "$TINOX" build "$f" -o "$TMP/ex" >/dev/null 2>&1; then ok; else bad; fi
done

echo "== Dogfood: examples Smoke-Runs =="
smoke() { # name file expected
    step "$2"
    local out
    "$TINOX" build "$2" -o "$TMP/smoke_$1" >/dev/null 2>&1 || { bad; return; }
    out=$(cd "$TMP" && timeout 10 "./smoke_$1" 2>&1)
    if [ "$out" = "$3" ]; then ok; else bad; fi
}
smoke simple examples/simple_test.tnx ""
smoke vtable examples/vtable_dispatch.tnx "$(printf '5\n10\n42')"
smoke modules examples/modules/main.tnx "$(printf '7\n12')"

echo "== Dogfood: benchmarks kompilieren =="
for f in benchmarks/*.tnx; do
    step "$f"
    if "$TINOX" build "$f" -o "$TMP/bench" >/dev/null 2>&1; then ok; else bad; fi
done

echo "== Dogfood: jgrep-tinox (${DOGFOOD_DIR}) =="
if [ -d "$DOGFOOD_DIR" ]; then
    step "build.sh"
    if (cd "$DOGFOOD_DIR" && PATH="$(dirname "$TINOX"):$PATH" bash build.sh >/dev/null 2>&1); then ok; else bad; fi
    for t in "$DOGFOOD_DIR"/tests/*_test.tnx; do
        step "$(basename "$t")"
        if (cd "$DOGFOOD_DIR" && PATH="$(dirname "$TINOX"):$PATH" timeout 180 tinox test "$t" >/dev/null 2>&1); then ok; else bad; fi
    done
else
    echo "  übersprungen ($DOGFOOD_DIR nicht gefunden)"
fi

if [ "$FAIL" -ne 0 ]; then
    echo
    echo "Dogfood FAILED — Details: Kommando von Hand wiederholen."
    exit 1
fi
echo
echo "Dogfood OK"
