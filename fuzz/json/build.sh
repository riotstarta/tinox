#!/usr/bin/env bash
# Builds fuzz/json/json_fuzzer. Requires a clang with libFuzzer support
# (compiler-rt's fuzzer runtime — the same clang tinox itself requires,
# no extra package needed; verified against clang 22 on this repo's dev
# machine).
set -euo pipefail
cd "$(dirname "$0")"

RUNTIME_C="../../runtime/runtime.c"
OUT="json_fuzzer"

# -DTINOX_NO_GC: same mode `make asan` uses (see Makefile) — plain
#   malloc/calloc instead of Boehm GC, so no GC_INIT() is required and
#   ASan can see every allocation. Leaks are expected/intentional in this
#   mode (nothing is ever freed) — see fuzz/README.md before running.
# -Dmain=tinox_runtime_unused_main: runtime.c defines its own main()
#   (calls into codegen'd tinox_main()), which would collide with
#   libFuzzer's main (from -fsanitize=fuzzer at link time). Renaming it
#   out of the way is simpler than patching runtime.c; the renamed
#   function is never called (json_fuzzer.cc stubs its tinox_main()/
#   __tinox_err dependencies so it still links).
clang -c "$RUNTIME_C" -o runtime_fuzz.o \
    -DTINOX_NO_GC -Dmain=tinox_runtime_unused_main \
    -fsanitize=fuzzer-no-link,address -g -O1

clang++ -fsanitize=fuzzer,address json_fuzzer.cc runtime_fuzz.o \
    -o "$OUT" -lm -lpthread

rm -f runtime_fuzz.o
echo "built fuzz/json/$OUT"
