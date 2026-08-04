#!/usr/bin/env bash
# Builds fuzz/zip/zip_fuzzer. See fuzz/json/build.sh for why each flag is
# needed (-DTINOX_NO_GC, -Dmain=..., fuzzer-no-link vs fuzzer) — identical
# reasoning, just a different fuzz target.
set -euo pipefail
cd "$(dirname "$0")"

RUNTIME_C="../../runtime/runtime.c"
OUT="zip_fuzzer"

clang -c "$RUNTIME_C" -o runtime_fuzz.o \
    -DTINOX_NO_GC -Dmain=tinox_runtime_unused_main \
    -fsanitize=fuzzer-no-link,address -g -O1

clang++ -fsanitize=fuzzer,address zip_fuzzer.cc runtime_fuzz.o \
    -o "$OUT" -lm -lpthread -lz

rm -f runtime_fuzz.o
echo "built fuzz/zip/$OUT"
