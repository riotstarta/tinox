# Changelog

All notable changes to Tinox are documented in this file.

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
