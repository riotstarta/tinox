#!/usr/bin/env bash
# Dogfood-Gate (TESTPLAN Phase 3): examples/ + benchmarks/ bauen (mit
# Smoke-Runs, wo deterministisch) und jgrep-tinox bauen + Tests fahren.
# Aufruf über `make dogfood`; erwartet ein frisches target/release/tinox.
#
# Alle Jobs sind voneinander unabhängig (jeder eigene tinox-Prozess, eigener
# Output-Pfad) und laufen deshalb parallel im Hintergrund; nur das
# Einsammeln/Ausgeben der Ergebnisse passiert danach sequenziell in der
# ursprünglichen, stabilen Reihenfolge. jgrep-tinox' `tinox test` schreibt
# PID-gescopte Temp-Dateien (.tinox_test_{pid}_{n}, s. main.rs), mehrere
# gleichzeitige Läufe im selben Checkout sind daher sicher. Vorher: ~4:45min
# fast rein sequenziell auf einer 32-Kern-Maschine.
set -uo pipefail
cd "$(dirname "$0")/.."

TINOX="$PWD/target/release/tinox"
DOGFOOD_DIR="${DOGFOOD_DIR:-../jgrep-tinox}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/out" "$TMP/status" "$TMP/log"

FAIL=0
step() { printf '  %-44s' "$1"; }
ok()   { echo "OK"; }
bad()  { echo "FAIL"; FAIL=1; }

# Sanitizes a label into a safe filename fragment for per-job status/log/output files.
job_id() { echo "$1" | tr -c 'A-Za-z0-9' '_'; }

# Launches "$@" in the background; exit code -> $TMP/status/<id>, combined
# output -> $TMP/log/<id>.
run_job() {
    local id="$1"; shift
    ( "$@" >"$TMP/log/$id" 2>&1; echo $? >"$TMP/status/$id" ) &
}

# Prints the step label followed by OK/FAIL, based on a previously run_job'd id.
report() {
    step "$1"
    if [ "$(cat "$TMP/status/$2" 2>/dev/null)" = "0" ]; then ok; else bad; fi
}

# Launches a build+run+compare-stdout job in the background (same check as
# the old sequential `smoke()`: exact stdout match, not just exit code).
# Writes 0/1 to $TMP/status/<id> itself instead of using run_job, since the
# pass/fail decision here is the string comparison, not a command's exit code.
run_smoke_job() { # id file expected
    local id="$1" file="$2" expected="$3"
    (
        if ! "$TINOX" build "$file" -o "$TMP/out/$id" >"$TMP/log/$id" 2>&1; then
            echo 1 >"$TMP/status/$id"
            exit
        fi
        out=$(cd "$TMP/out" && timeout 10 "./$id" 2>&1)
        if [ "$out" = "$expected" ]; then
            echo 0 >"$TMP/status/$id"
        else
            { echo "expected: $expected"; echo "actual:   $out"; } >>"$TMP/log/$id"
            echo 1 >"$TMP/status/$id"
        fi
    ) &
}

GOOD_EXAMPLES=(
    examples/examples.tnx
    examples/cli_test.tnx
    examples/simple_test.tnx
    examples/vtable_dispatch.tnx
    examples/rest_minimal.tnx
    examples/rest_test.tnx
    examples/modules/main.tnx
    examples/modules/multi_import.tnx
    examples/interface_extends.tnx
    examples/rest_with_mini.tnx
)
for f in "${GOOD_EXAMPLES[@]}"; do
    run_job "$(job_id "build_$f")" "$TINOX" build "$f" -o "$TMP/out/$(job_id "build_$f")"
done

run_smoke_job "$(job_id smoke_simple)" examples/simple_test.tnx ""
run_smoke_job "$(job_id smoke_vtable)" examples/vtable_dispatch.tnx "$(printf '5\n10\n42')"
run_smoke_job "$(job_id smoke_modules)" examples/modules/main.tnx "$(printf '7\n12')"
run_smoke_job "$(job_id smoke_multiimport)" examples/modules/multi_import.tnx "$(printf '25\n30')"
run_smoke_job "$(job_id smoke_ifaceext)" examples/interface_extends.tnx "42"

run_job "$(job_id mini_http_check)" "$TINOX" check examples/mini_http.tnx

for f in benchmarks/*.tnx; do
    run_job "$(job_id "bench_$f")" "$TINOX" build "$f" -o "$TMP/out/$(job_id "bench_$f")"
done

if [ -d "$DOGFOOD_DIR" ]; then
    run_job "$(job_id jgrep_build)" bash -c "cd '$DOGFOOD_DIR' && PATH='$(dirname "$TINOX")':\"\$PATH\" bash build.sh"
    for t in "$DOGFOOD_DIR"/tests/*_test.tnx; do
        run_job "$(job_id "jgrep_test_$t")" bash -c "cd '$DOGFOOD_DIR' && PATH='$(dirname "$TINOX")':\"\$PATH\" timeout 180 tinox test '$t'"
    done
fi

wait

echo "== Dogfood: examples bauen =="
for f in "${GOOD_EXAMPLES[@]}"; do
    report "$f" "$(job_id "build_$f")"
done

echo "== Dogfood: examples Smoke-Runs =="
report examples/simple_test.tnx "$(job_id smoke_simple)"
report examples/vtable_dispatch.tnx "$(job_id smoke_vtable)"
report examples/modules/main.tnx "$(job_id smoke_modules)"
report examples/modules/multi_import.tnx "$(job_id smoke_multiimport)"
report examples/interface_extends.tnx "$(job_id smoke_ifaceext)"

echo "== Dogfood: Library-Beispiele typechecken =="
report "examples/mini_http.tnx (check)" "$(job_id mini_http_check)"

echo "== Dogfood: benchmarks kompilieren =="
for f in benchmarks/*.tnx; do
    report "$f" "$(job_id "bench_$f")"
done

echo "== Dogfood: jgrep-tinox (${DOGFOOD_DIR}) =="
if [ -d "$DOGFOOD_DIR" ]; then
    report "build.sh" "$(job_id jgrep_build)"
    for t in "$DOGFOOD_DIR"/tests/*_test.tnx; do
        report "$(basename "$t")" "$(job_id "jgrep_test_$t")"
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
