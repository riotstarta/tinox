# Fuzzing

Coverage-guided (`libFuzzer`, via clang's `-fsanitize=fuzzer`) fuzz
harnesses for tinox stdlib functions that parse untrusted input. Added for
[#111](https://github.com/subnix-work/tinox/issues/111): the v1.0.2
security hardening pass (issues #86–#108) fixed ~20 bugs almost all in the
same shape — a parser trusted broker/client/file-controlled input without
bounds checks (malformed AMQP frames, an unbounded SASL iteration count, a
JSON parser infinite loop, a ZIP reader trusting a declared uncompressed
size independent of the compressed size). That bug class is exactly what
coverage-guided fuzzing finds automatically instead of via one-off manual
review.

No new tooling is required: `clang`/`clang++` already ship libFuzzer
support as part of compiler-rt, and tinox already requires clang. Verified
against clang 22.1.8.

## Targets

| Target | `fuzz/<target>/` | What it calls | Input bridging |
|---|---|---|---|
| JSON parser | `json/` | `jsonParse(const char*)` directly | none — `jsonParse` already takes a raw buffer |
| ZIP reader | `zip/` | `zipEntryCount(const char*)` → `tinox_zip_parse()` | writes each input to an anonymous `O_TMPFILE`, passes `/proc/self/fd/<n>` as the path (the zip\*() functions are path-based, not buffer-based) |
| HPACK decoder | `hpack/` | `Hpack::decode(List<Int64>, HpackDynTable)` (`crates/tinox-core/hpack/Hpack.tnx`) | `hpack_driver.tnx` imports the real module and adds a one-line `tinoxHpackDecode` wrapper; `build.sh` compiles it via the real `tinox build` to LLVM IR, then recompiles that IR with ASan/coverage instrumentation — see "HPACK: calling compiled Tinox code" below |

JSON and ZIP call straight into the real `runtime/runtime.c`; HPACK calls
straight into the real, compiled `crates/tinox-core/hpack/Hpack.tnx` — none
of the three are a copy of the parsing logic, so a fix or a regression in
either is picked up automatically.

### HPACK: calling compiled Tinox code, not C

`amqp091`/`amqp10`/`http2_server`/`hpack` are all written in Tinox itself
(`crates/tinox-core/**/*.tnx`), not C — there's no `parse(bytes)`-shaped
function in `runtime.c` to call directly the way JSON/ZIP do. HPACK's
`Hpack::decode` turned out to be the tractable one of the four: it's a pure
`(List<Int64>, HpackDynTable) -> List<HpackHeader>` function with no socket
dependency, unlike AMQP/HTTP2 frame parsing (see "Extending to other
parsers" below).

To fuzz it: `fuzz/hpack/hpack_driver.tnx` imports `tinox.core.hpack` and
adds one wrapper function, `tinoxHpackDecode(bytes: List<Int64>) -> Int64`.
`tinox build` compiles this down to plain LLVM IR with a predictable
exported symbol (`@tinoxHpackDecode`, `i64* -> i64` — Tinox top-level
functions keep their literal name, and a `List<Int64>` value is just an
`i64*` pointer to the `{len, cap, data}` handle `runtime.c`'s `TinoxArray`
uses). `tinox build` always tries to link a full executable and fails at
that final step (`hpack_driver.tnx` has no `main()`, so there's no
`tinox_main` for `runtime.c`'s `main()` to call) — that failure is expected
and harmless; `build.sh` only needs the `.ll` it leaves behind before the
failing link. `build.sh` then recompiles that IR with
`-fsanitize=fuzzer-no-link,address` (`tinox build`'s own clang/opt/llc
pipeline adds neither ASan nor coverage instrumentation) and links it with
an ASan-instrumented `runtime.c` (renamed `main`, same as JSON/ZIP) plus
`hpack_fuzzer.cc`, which builds the input `TinoxArray` directly and calls
`tinoxHpackDecode`.

This generalizes to any other buffer-in/buffer-out Tinox stdlib function,
not just HPACK — the driver-module + recompiled-IR pattern doesn't care
what the function does, only that it takes/returns primitives or handles
with a stable, C-callable ABI.

## Building and running

```bash
fuzz/json/build.sh  && fuzz/json/json_fuzzer   fuzz/json/corpus/  fuzz/json/seeds/
fuzz/zip/build.sh   && fuzz/zip/zip_fuzzer     fuzz/zip/corpus/   fuzz/zip/seeds/
fuzz/hpack/build.sh && fuzz/hpack/hpack_fuzzer fuzz/hpack/corpus/ fuzz/hpack/seeds/
```

`corpus/` is the fuzzer's working corpus (gitignored — it grows large and
is reproducible from `seeds/` plus a fuzzing run); `seeds/` is a small,
curated set of valid/edge-case inputs checked into git as a starting
point. Create `corpus/` before the first run (`mkdir -p fuzz/<target>/corpus`);
libFuzzer does not create its output directory for you.

A crashing/hanging/leaking input gets written to the current directory by
default (or under `-artifact_prefix=fuzz/<target>/crashes/`, also
gitignored). Reproduce and debug one with:

```bash
fuzz/json/json_fuzzer fuzz/json/crashes/crash-<hash>
```

## All targets run in `-DTINOX_NO_GC` mode — leaks are intentional

Every harness builds `runtime.c` with `-DTINOX_NO_GC` (see each
`build.sh`), the same mode `make asan` uses: plain `malloc`/`calloc`
instead of the Boehm GC, so ASan can see every allocation and no
`GC_INIT()` call is required. As the existing comment on that `#ifdef` in
`runtime.c` says, nothing is ever freed in this mode — that's deliberate,
matching `make asan`'s `ASAN_OPTIONS="detect_leaks=0"` (see `Makefile`).
**Always pass `-detect_leaks=0` to the fuzzer binary** (both as an
`ASAN_OPTIONS` env var and a libFuzzer flag — the flag alone isn't
enough, LeakSanitizer runs as part of the ASan runtime) or every run will
immediately report a "leak" that is not a bug.

Because nothing is freed, RSS grows monotonically over a single-process
run — empirically this hits libFuzzer's default 2048 MB `-rss_limit_mb`
after roughly 230k executions (~15s of wall time on this repo's dev
machine, ~50k exec/s). That OOM is an artifact of the harness, not a
finding. For a run longer than a couple of minutes, either:

- pass a lower `-rss_limit_mb` and treat hitting it as "time to restart,"
  or
- use libFuzzer's `-fork=N` mode, which periodically respawns worker
  processes (reclaiming their memory) while still sharing one corpus —
  the practical way to fuzz for hours unattended.

## What the HPACK target found

Within the first second of running `fuzz/hpack/hpack_fuzzer` against its
seed corpus, it reproduced a crash: a truncated/malformed HPACK header
block (as short as 3 bytes) made `Hpack::decodeInt`/`decodeStr` index past
the end of the input `List<Int64>`, which `tinox_array_get`'s bounds check
turns into `exit(1)` — i.e. the *entire process* exits, not just the one
malformed request. Since `Http2Server.dispatchStream` calls
`Hpack::decode` directly on client-controlled `stream.headerBlock` with no
validation beforehand, this was a remotely-triggerable denial of service
against any Tinox HTTP/2 server: one connection sending a truncated HPACK
literal took down every other connection's process too. Fixed by bounds-
checking `decodeInt`'s entry and `decodeStr`'s consuming loop against
`data.len()` (stopping gracefully instead of reading past the buffer) and
capping `decodeInt`'s continuation-byte loop so a long run of
`0x80`-prefixed bytes can't shift-overflow past 64 bits — see
`crates/tinox-core/hpack/Hpack.tnx` and the `tests/e2e/hpack_truncated_input.tnx`
regression test. Exactly the bug class this issue set out to catch.

## Extending to other parsers

The original issue also flagged `amqp091`/`amqp10` frame parsing and the
HTTP/2 frame parser as candidates, beyond HPACK above:

- **ZIP and JSON were the easy cases** because their parse entry points
  (`jsonParse`, `zipEntryCount`/`tinox_zip_parse`) are plain C functions
  taking a buffer or a path — call them directly (JSON) or bridge via a
  temp file (ZIP), no different from any other libFuzzer target.
- **HPACK is Tinox source, not C, but still buffer-in/buffer-out** — see
  "HPACK: calling compiled Tinox code, not C" above for how a driver
  module plus a recompiled-IR step bridges that.
- **AMQP-0-9-1/1.0 and HTTP/2 frame parsing are not standalone
  functions** — they read directly from a live socket fd in the middle of
  the connection state machine (`conn_recv`/`conn_send` and friends), not
  from an in-memory buffer handed to a `parse(bytes)`-shaped entry point
  the way `Hpack::decode` is. Fuzzing them means feeding fixed bytes
  through something that looks like a socket to that code, e.g. a
  `socketpair()` where a second thread (or a pre-filled kernel send
  buffer) supplies the fuzz input on the read end, driving a compiled
  Tinox connection-handler entry point (reachable the same way
  `tinoxHpackDecode` is here) rather than a single pure function. That's a
  meaningfully bigger harness than any target here and is left out of
  this pass — tracked as follow-up work in
  [#111](https://github.com/subnix-work/tinox/issues/111) rather than
  attempted alongside it, per this project's "gezielt statt pauschal
  fixen" convention (CLAUDE.md).
