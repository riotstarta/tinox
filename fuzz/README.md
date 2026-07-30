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

Both call straight into the real `runtime/runtime.c` — not a copy of the
parsing logic — so a fix or a regression in `runtime.c` is picked up
automatically; there is nothing here to keep in sync by hand.

## Building and running

```bash
fuzz/json/build.sh && fuzz/json/json_fuzzer fuzz/json/corpus/ fuzz/json/seeds/
fuzz/zip/build.sh  && fuzz/zip/zip_fuzzer  fuzz/zip/corpus/  fuzz/zip/seeds/
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

## Both targets run in `-DTINOX_NO_GC` mode — leaks are intentional

Both harnesses build `runtime.c` with `-DTINOX_NO_GC` (see each
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

## Extending to other parsers

The original issue also flagged `amqp091`/`amqp10` frame parsing and the
HTTP/2 frame parser/HPACK decoder as candidates. Investigated as part of
building the two targets above:

- **ZIP and JSON were the easy cases** because their parse entry points
  (`jsonParse`, `zipEntryCount`/`tinox_zip_parse`) are plain C functions
  taking a buffer or a path — call them directly (JSON) or bridge via a
  temp file (ZIP), no different from any other libFuzzer target.
- **AMQP-0-9-1/1.0 and HTTP/2 frame parsing are not standalone
  functions** — they read directly from a live socket fd in the middle of
  the connection state machine (`conn_recv`/`conn_send` and friends), not
  from an in-memory buffer handed to a `parse(bytes)`-shaped entry point.
  Fuzzing them means feeding fixed bytes through something that looks
  like a socket to that code, e.g. a `socketpair()` where a second thread
  (or a pre-filled kernel send buffer) supplies the fuzz input on the
  read end. That's a meaningfully bigger harness than either target here
  and was left out of this first pass — tracked as follow-up work in
  [#111](https://github.com/subnix-work/tinox/issues/111) rather than
  attempted alongside it, per this project's "gezielt statt pauschal
  fixen" convention (CLAUDE.md).
