# Changelog

All notable changes to Tinox are documented in this file.

## [2.0.0] - 2026-08-02

### Breaking

- **Mandatory class-qualified function calls** ([#149](https://github.com/subnix-work/tinox/issues/149)).
  A top-level `fn`/`fnc` declaration with a body is now a hard compile
  error (`extern fn` FFI declarations stay legal). Every program needs an
  entry point shaped exactly `class Main { fnc main() -> Int32 }`, living
  in a file named `Main.tnx` (per the existing one-class-per-file rule).
  A same-class bare call (`helper()` from another method of the same
  class) is resolved automatically; a call into a *different* class still
  needs the `ClassName::method()` form. Existing programs using top-level
  `fn` need migrating — wrap the entry point and any free helper
  functions in a class.

### Fixed

- **Memory safety**: `tinox_HttpServer_listen`'s epoll worker threads
  could crash inside GC-managed memory under allocation-heavy
  annotation-driven (`@GET`/`@POST`/…) routes
  ([#140](https://github.com/subnix-work/tinox/issues/140)). Root cause:
  several `static __thread` runtime buffers hold pointers to GC-managed
  memory, but Boehm GC does not automatically scan thread-local storage
  as roots — now explicitly registered via `GC_add_roots()` on every
  thread that can run Tinox code.
- `Map<K, V>` with a non-`String` key (`Int64`, `Bool`, …) segfaulted
  immediately on `insert`/`get`/`contains`/`remove` and on `m[key]`
  indexing — the key's raw bit pattern was reinterpreted as a pointer
  instead of being stringified
  ([#129](https://github.com/subnix-work/tinox/issues/129)).
- A struct literal omitting a declared field (or naming an unknown one)
  silently left the field as uninitialized garbage instead of failing to
  compile
  ([#130](https://github.com/subnix-work/tinox/issues/130)).
- Two classes sharing a bare name across different imported modules
  silently corrupted each other's codegen field layout, surfacing as a
  confusing "field not in layout of typed class" internal error; now a
  clear compile-time diagnostic
  ([#139](https://github.com/subnix-work/tinox/issues/139)).
- An `Int32`-returning call result used in a binary expression and
  stored into an `Int32` local generated invalid LLVM IR (an `i64`/`i32`
  type mismatch)
  ([#150](https://github.com/subnix-work/tinox/issues/150)).
- `resolve_entry_file` ignored `tinox.toml`'s `[package] entry` field,
  always falling back to the hardcoded `src/main.tnx`
  ([#152](https://github.com/subnix-work/tinox/issues/152)).
- `fuzz/{hpack,amqp091,amqp10}/build.sh` failed to link, missing `-lz`
  ([#151](https://github.com/subnix-work/tinox/issues/151)).
- Verified the `@Auth` annotation's credential-validation fix (default-
  deny without a registered `AuthValidator`) actually holds
  ([#141](https://github.com/subnix-work/tinox/issues/141)).

## [1.0.2] - 2026-07-28

Security and robustness hardening. 23 issues found by an automated
security review and independently verified against current source
before fixing — see [#86](https://github.com/subnix-work/tinox/issues/86)
through [#108](https://github.com/subnix-work/tinox/issues/108) for full
root-cause/fix/verification details on each. No breaking changes.

### Fixed

- **AMQP 1.0 / 0-9-1**: `Amqp10Reader`/`AmqpReader091` had no bounds
  checking, so a malformed or truncated broker frame (including a
  heartbeat reused as a SASL frame) crashed the client
  ([#86](https://github.com/subnix-work/tinox/issues/86),
  [#89](https://github.com/subnix-work/tinox/issues/89)). SCRAM-SHA-256's
  broker-supplied iteration count had no upper bound, enabling a CPU-DoS
  during connect
  ([#87](https://github.com/subnix-work/tinox/issues/87)).
  `Amqp10Connection::connect()` had no TLS variant, so SASL
  credentials went out in cleartext — added `connectTls()`
  ([#88](https://github.com/subnix-work/tinox/issues/88)). Failed dials
  leaked the socket fd
  ([#90](https://github.com/subnix-work/tinox/issues/90)).
- **WebSocket / HTTP / HTTP/2**: TLS accept had no receive timeout, so
  one stalled client could hang the whole HTTPS/WSS server
  ([#91](https://github.com/subnix-work/tinox/issues/91)). WebSocket
  control frames (Ping/Pong/Close) didn't enforce RFC 6455's
  FIN/length limits
  ([#92](https://github.com/subnix-work/tinox/issues/92)). An HTTP/2
  stream's header block/body could grow without bound across
  CONTINUATION/DATA frames
  ([#94](https://github.com/subnix-work/tinox/issues/94)). The response
  header buffer could overflow on the stack
  ([#95](https://github.com/subnix-work/tinox/issues/95)), and
  Content-Length had no cap
  ([#96](https://github.com/subnix-work/tinox/issues/96)).
- **Runtime memory safety**: an integer overflow in the array allocator
  could corrupt the heap for large peer-controlled lengths
  ([#93](https://github.com/subnix-work/tinox/issues/93)). The JSON
  parser could loop forever on malformed input
  ([#97](https://github.com/subnix-work/tinox/issues/97)). The ZIP
  reader trusted an entry's uncompressed size independent of its
  compressed size, allowing an out-of-bounds read from a crafted archive
  ([#98](https://github.com/subnix-work/tinox/issues/98)). The
  Prometheus metrics formatter could overflow its output buffer with
  long metric names
  ([#99](https://github.com/subnix-work/tinox/issues/99)).
- **Package manager**: `tinox install` used a dependency's
  group/artifactId/version directly as filesystem path components,
  allowing a malicious `tinox.yaml` to write files outside
  `.tinox/deps` (critical)
  ([#100](https://github.com/subnix-work/tinox/issues/100)).
- **Concurrency**: the cross-function exception slot
  (`@__tinox_err`) was a process-wide global instead of thread-local,
  so concurrent HTTP request handlers could race on each other's
  thrown/caught values — now `thread_local`
  ([#101](https://github.com/subnix-work/tinox/issues/101)). The
  SQLite statement cache and the Postgres connection had no locking
  despite the HTTP server running handlers concurrently
  ([#102](https://github.com/subnix-work/tinox/issues/102),
  [#103](https://github.com/subnix-work/tinox/issues/103)).
- **Auth / REST framework**: `@Auth("bearer"/"basic")` only checked
  the Authorization header's scheme prefix and never validated the
  actual credential — it now fails closed by default and requires the
  application to register a real validator via the new
  `RestApi::setAuthValidator()`
  ([#104](https://github.com/subnix-work/tinox/issues/104)).
  `Jwt::verify()`/`decode()` ignored `exp`/`nbf` claims, accepting
  expired tokens
  ([#105](https://github.com/subnix-work/tinox/issues/105)).
  `Http::setHeader()` had no CRLF validation, allowing header
  injection from `RestClient`/`RequestBuilder`
  ([#106](https://github.com/subnix-work/tinox/issues/106)). Generated
  `toString()`/`toJson()` keyed the `@Sensitive`/`@Masked`/
  `@DoNotSerialize` skip-set by the wrong class, so an inherited
  sensitive field leaked in a subclass's output
  ([#107](https://github.com/subnix-work/tinox/issues/107)).
- **CI**: the `jgrep-tinox` dogfood checkout was unpinned and the
  workflow had no explicit least-privilege `permissions:` — now pinned
  to a commit SHA with `contents: read`
  ([#108](https://github.com/subnix-work/tinox/issues/108)).

### Added

- `Amqp10Connection::connectTls(host, port, user, pass, verify)` — TLS
  variant of `connect()`.
- `RestApi::setAuthValidator(fnc(String, String) -> Bool)` — registers
  the credential validator used by `@Auth`-protected routes.

## [1.0.1] - 2026-07-27

Packaging fix, no language/stdlib changes.

### Fixed

- The `tinox` binary was not relocatable: `tinox build`/`tinox run`
  located `runtime.c` via an absolute path baked in at Rust compile time
  (`CARGO_MANIFEST_DIR`), with no fallback or override. On any machine
  other than the one it was built on, every build failed
  ("Runtime compilation failed"). Standard library imports
  (`import tinox.core.*` — used by nearly every non-trivial program) had
  a partial fix already (`TINOX_PATH` env var) but no system-install
  fallback either, failing with `"TINOX_PATH not set and dev path not
  found"`.

  Both now additionally fall back to a fixed system path
  (`/usr/share/tinox/runtime.c` and `/usr/share/tinox/core`) after the
  existing `TINOX_PATH`/dev-checkout checks, so a distro-packaged
  `tinox` binary (e.g. the AUR `tinox-bin` package, which installs to
  those paths) works standalone. See
  [#85](https://github.com/subnix-work/tinox/issues/85).

## [1.0.0] - 2026-07-27

First official release. Tinox is a native, statically typed programming
language with an LLVM backend, garbage collection, and concurrency
support.

### Language

- Lexer with Unicode support, string interpolation, and ranges (`..` / `...`).
- Full recursive-descent parser producing a complete AST.
- Static type checker: base types, classes, interfaces, enums, generics
  (monomorphized), function types, annotations.
- LLVM IR code generator, including `@inline` support and a typed
  value bridge between typecheck and codegen.
- C runtime (pthread-based): `spawn` starts a real POSIX thread, not a
  cooperative coroutine.
- Classes with inheritance, interfaces with vtable dispatch, enums with
  pattern matching, generics, tuples, arrays, lambdas/closures, `Map`,
  `defer`, `try`/`catch`/`finally`/`throw`, an import system, and an
  annotation system (`@Name`/`@Name(args)`) validated by the compiler.
- Async/concurrency: `spawn`/`await`, channels, `select`.
- **Hard compiler rule (new in this release):** a `.tnx` file may contain
  at most one top-level `class`/`interface`/`enum`; if it has one, the
  filename must match the type name exactly. Multi-type modules are
  directories with one file per type.

### CLI

`tinox build` / `run` / `check` / `fmt` / `fmt --write`. `--checked` build
mode enables a heap-kind registry that guards array/map runtime functions
against dispatch-on-wrong-type bugs.

### Standard Library (`tinox-core`, 50+ modules)

- **HTTP:** `http_server` (incl. `listenTls` for HTTPS), `rest_framework`
  (annotation-driven REST controllers: `@GET`/`@POST`/`@PUT`/`@PATCH`/
  `@DELETE`, `@Path`, `@Produces`, `@Consumes`, `@StatusCode`, `@Auth`),
  `mini_http`.
- **WebSocket (v1):** RFC 6455 server (`websocket`), `wss://` (TLS), and
  an annotation-driven form (`@WebsocketEndpoint`/`@OnOpen`/`@OnMessage`/
  `@OnClose`). Known v1 gaps: no fragmentation, no client, no
  permessage-deflate.
- **AMQP-0-9-1 client (v1):** `amqp091` for brokers such as RabbitMQ,
  including `amqps://` (TLS). Known v1 gaps: no multi-channel, no
  `exchange.declare` (default/broker-predefined exchanges only), no
  publisher confirms, no annotation-driven consumer API, no
  heartbeat/auto-reconnect.
- **AMQP-1.0 client:** `amqp10`, a separate implementation (own type
  system, Connection→Session→Link hierarchy with credit-based flow
  control). Covers multiple sessions/links per connection, SASL PLAIN
  and SCRAM-SHA-256, delivery states beyond `accepted` (`rejected`/
  `released`/`modified`), transactions, link recovery/resumption,
  heartbeat/auto-reconnect, and an annotation-driven consumer API
  (`@Amqp10Consumer`/`@OnMessage`).
- **Data:** `json`, `csv`, `xml`, `yaml`, `toml`, `regex`, `base64`, `hex`.
- **Security:** `crypto` (incl. AES, PBKDF2/HMAC-SHA-256), `jwt`.
- **Persistence:** SQLite-backed ORM (`db`, `@Entity`), CRUD example app.
- **Collections/utilities:** `collections`, `queue`, `stack`, `heap`,
  `trie`, `graph`, `bitmap`, `set`, `cache`/`LruCache`, `math`/`mathf`/
  `mathx`, `string`, `time`/`duration`, `uuid`, `random`, `decimal`,
  `complex`.
- **System/async:** `fs`, `io`, `env`, `process`, `socket`, `cron`,
  `events` (EventEmitter), `pool`, `semaphore` (Mutex/RWLock),
  `ratelimit`, `metrics`.
- **Protocol/format internals:** `hpack` (HTTP/2 header compression),
  `http2_server`.

### Tooling

- Language Server Protocol implementation (`tinox-lsp`).
- Eclipse plugin.
- `tinox fmt` formatter.
- REPL: not yet implemented (planned).

### Testing & CI

- Golden end-to-end tests (`tests/e2e/*.tnx`), a generated context
  matrix, a boundary-value suite, and a stdlib smoke gate exercising
  every `tinox-core` module.
- `make check` (clippy with zero warnings, unit tests, e2e/matrix/
  boundary/stdlib_smoke, and a dogfood pass building examples/
  benchmarks and a downstream project) gates every commit via a
  pre-push hook and GitHub Actions.
- `make asan` (AddressSanitizer, plain malloc) and `make checked`
  (heap-kind registry) as periodic/pre-release deep-verification passes,
  outside the default `make check` gate.

### Known limitations in this release

- [#84](https://github.com/subnix-work/tinox/issues/84): `DB.of(EntityClass).save(...)`
  / `.delete(...)` are not recognized as ORM query-chain terminals and
  currently fail to compile (invalid LLVM IR) rather than silently
  misbehaving. Affects `examples/crud` only; not exercised by any
  `tests/e2e/orm_sqlite_*.tnx` test, which use `filter`/`count`/`first`/
  `list` instead.
- See the README sections on the WebSocket and AMQP-0-9-1 clients above
  for their documented v1 scope limitations.

### Project history

This release consolidates over 250 commits and 80+ tracked issues
(bug fixes and feature work) since the initial compiler bring-up. Full
history: [GitHub issues](https://github.com/subnix-work/tinox/issues?q=is%3Aissue).
