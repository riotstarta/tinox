# Project Conventions for Claude Code

## Bug/Feature Tracking Runs Through GitHub Issues

Since 2026-07-25, **all** bugs and completed feature implementations are
tracked as [GitHub Issues](https://github.com/subnix-work/tinox/issues)
on `subnix-work/tinox` — no longer in local Markdown files
(`bugs.md`/`bugs_fixed.md` were removed, their full content lives 1:1
in issues #1–74).

**Binding rule for every new find/fix from now on:**

- **New bug found** (whether fixed immediately or not): open a GitHub
  issue directly (`gh issue create --repo subnix-work/tinox`).
  Title in the style `Bug NN — short description` (sequential number,
  continuing from the last assigned issue number) or `Feature: Name`
  for completed feature work. Label `bug` or `enhancement`.
- **Bug is fixed:** the issue body contains (as previously used in
  bugs.md/bugs_fixed.md) Status/Root Cause/Fix/Verified — then close the
  issue (`gh issue close <NR> --reason completed`).
- **Bug is still open** (deliberately deferred or unresolved): the issue
  stays open, the body describes the current state + why it's open.
- **Language: English** (title + body) — the issues were deliberately
  translated to English and are meant to stay that way (since
  2026-07-26 this applies project-wide to commit messages and code too,
  see below anyway).
- Cross-references between related issues as previously used between
  bug entries (e.g. "closes what Bug 40 left open" with a link to the
  issue number).

**Looking up history:** search/filter in the GitHub issues (open vs.
closed, label, full text), not in a local file. **Careful:** the old
"Bug NN" number from before the migration does **NOT** reliably match
the same issue number (e.g. "Bug 40" is actually issue #41 — an
in-between note heading without a bug number shifted the count, and
several bugs were sometimes merged into a single issue, e.g. "Bugs
64–65" including the embedded bugs 66–71 as ONE issue). Always search
by title (`gh issue list --repo subnix-work/tinox --state all --search
"Bug 40"`), not by assumed number.

## Core Philosophy (distilled from 70+ documented bugs)

- **No silent garbage.** Every error case gets a hard, visible failure
  instead of silent data corruption or a quiet default value. When in
  doubt: abort hard with a clear message instead of "mostly works
  somehow". This is by far the most common root-cause category across
  the whole bug log.
- **Verify against real, independent systems, not just
  self-consistent tests.** Simulated broker/server tests (via
  `spawn`/`await`) are necessary and good, but structurally find NO
  bugs where the implementation is self-consistent but wrong (e.g. bug
  70/71: the `initial-delivery-count` mandatory field and the
  `amqp-value`-vs-`data` encoding were only found through live tests
  against real RabbitMQ and an independent Python client). For
  network/protocol features: whenever at all possible, additionally
  verify against a real, third-party implementation.
- **Fix narrowly, not broadly.** For a found bug, choose the smallest,
  well-scoped fix instead of forcing a larger, riskier rewrite — even
  if the "clean" rewrite would be theoretically appealing. Known,
  documented design limits (open issues) are an acceptable outcome
  when the full fix would be disproportionately invasive.
- **Before tackling an "open" item: check whether a LATER fix already
  closed it.** Several times in the history, an entry said "still
  open" when it had already been resolved by the very next entry in
  the same log (e.g. Bug 35's remaining weakness → fixed in Bug 40; a
  `.toString()` finding in Bug 38 → fixed in Bug 39). Reproduce first,
  then invest time.

## Design preference: annotations over manual boilerplate

When working on tinox — whether extending the language itself or writing
examples/docs — prefer a declarative, annotation-based solution over
hand-written imperative wiring, if both are reasonably possible. This
applies to both directions:

- **Language design:** when adding a new capability that involves
  boilerplate-y setup/wiring (routing, lifecycle hooks, auth/role checks,
  serialization, endpoint registration, etc.), design it as an annotation
  (`@Http3RestController`, `@WebsocketEndpoint`, `@OnOpen`, OIDC role
  guards, …) that generates/wires the code, rather than requiring the
  user to write that plumbing by hand. Existing annotation-driven features
  are the template to follow.
- **Examples:** when writing or updating example code (`examples/**`),
  demonstrate the annotation-based way of doing something rather than the
  manual/imperative equivalent, whenever an annotation for it exists.

Only fall back to the manual/imperative approach when no annotation
exists for the capability yet, the annotation route would be
disproportionately invasive for the size of the change, or the example's
whole point is to show the manual/low-level mechanism itself.

## Build & Test

- `make check` (clippy + unit tests + e2e/matrix/boundary/stdlib_smoke
  twice + dogfood incl. `jgrep-tinox`) must be fully green before every
  commit — takes 15–25 minutes. Run it in the background (`nohup ... &
  disown`, poll the log file), don't wait on it blockingly. A failure is
  a REAL regression by default, not assumed flakiness — but: plain
  bind/port errors in e2e tests can arise from port collisions between
  two test files (has happened before), a quick `grep -rn
  "httpServerCreate(4" tests/` check is worth it before writing it off
  as "just flaky".
- `make asan` (AddressSanitizer, `-DTINOX_NO_GC`) and `make checked`
  (heap-kind registry, `-DTINOX_CHECKED`) are NOT part of `make check`,
  but useful when memory errors/dispatch bugs on the wrong heap-object
  type are suspected — per the Makefile comment, intended for
  weekly/pre-release runs.
- New e2e tests under `tests/e2e/*.tnx` with `// expect:` directives
  (line-by-line comparison of stdout output). For tests that bind a
  port: pick an actually free port (`grep -rn "httpServerCreate(4"
  tests/e2e/*.tnx examples/*.tnx` shows the ones already in use).
- Tests that use `spawn`/`await` (simulated broker/server via loopback)
  should be run 15–40× repeatedly before being trusted as green — the
  async runtime has had several timing-dependent bugs (Bug 68 among
  others) that only showed up on repeated runs.
- **Since 2026-07-26: commit messages AND code (incl. comments,
  identifiers, doc strings) are in English** — both in this repo and in
  downstream projects like jgrep-tinox. Older commits/comments stay in
  German (not changed retroactively, only new work is affected). The
  previous convention (commit messages in German in the style of the
  old bugs.md entries: Root Cause, Fix, Verified) is thereby replaced —
  the structure/content of the commit message stays the same, only the
  language changes.
- **`docs.html` (German) and `docs_en.html` (English) are deliberately
  maintained as parallel duplicates** — whenever a new `<div
  class="mod-section">` is added to `docs.html` (a new stdlib module),
  ALWAYS also add it to `docs_en.html` (nav link, overview card if
  present, translated module section). This was already out of sync
  for weeks since May once (WebSocket/AMQP-091/AMQP-1.0 were missing
  from the EN version until 2026-07-25) — don't let it happen again.
  Quick check when in doubt: `grep -oE 'id="mod-[a-z0-9_]+"' docs.html
  | sort -u` diffed against the same line for `docs_en.html`, must be
  empty.

## Every tinox-central Publish Needs a Matching Per-Version Doc Page

Whenever a `crates/tinox-core-ext/<module>` (or `tinox-core`) package is
published to tinox-central as a new version (`scripts/publish-stdlib-ext.sh`
or any manual `POST /api/v1/{group}/{artifactId}/{version}`), generate its
`tinox doc` page into `docs/<group-with-dots-as-dashes>/<artifactId>/
<version-with-dots-as-dashes>/docs.html` in THIS repo (e.g. `tinox.core` +
`amqp091` + `1.0.2` → `docs/tinox-core/amqp091/1-0-2/docs.html`) and commit
it alongside the version bump. **Never overwrite an existing version's
docs.html** — old versions stay published and browsable, so their doc page
must stay too; only ADD the new version's directory.

**Why:** tinox-central's frontend (`registry-frontend/.../
DocsProxyResource.java` + `RegistryClient.java` in the `tinox-central` repo)
has no docs of its own — it fetches this exact path from
`raw.githubusercontent.com/subnix-work/tinox/refs/heads/main/docs/...` and
proxies it into the package detail page's iframe. A published version
without a matching doc directory here just 404s in that iframe — this
already happened for 13 modules bumped in the 2026-08-10 core/extended
split's republish (amqp091, amqp10, crypto, http, http2_server,
http3_server, http_server, jwt, oauth2, oidc, rest, websocket, zip all
gained a new version with no doc page to match, silently, since nothing
enforces this link).

**How to generate one:** `tinox doc` only auto-discovers files under a
project's `src/` next to its `tinox.toml` (for the Description/Dependencies
sections) — but `crates/tinox-core-ext/<module>/` is flat (`.tnx` files
and `tinox.toml` directly in the module dir, no `src/`, matching the live
archive layout `publish-stdlib-ext.sh` uploads). So stage a throwaway
project first: create a temp dir, copy the module's `tinox.toml` in as-is
and copy its `.tnx` file(s) into a `src/` subdirectory (recursively for
multi-directory modules like `rest`'s `client/`/`server/`), then run
`tinox doc --out <path-to-repo>/docs/<group>/<artifactId>/<version>/
docs.html` from inside that staged dir. The Dependencies section is read
straight from the copied `tinox.toml`'s `[[dependencies]]` and links to
`../../<artifactId>/<version>/docs.html` — verify those targets actually
exist (they should, since dependencies are published/versioned first).

**Examples live in `docs/tinox-core/<module>/examples/*.tnx`, NOT inside
`crates/tinox-core-ext/<module>/`.** They used to sit in an `examples/`
dir next to the module's own source pre-split, but `publish-stdlib-ext.sh`
archives every `.tnx` it finds recursively under the module dir straight
into the published package — an `examples/` folder placed there would ship
inside the artifact itself and get pulled into every consumer's import
(and an example's own `class Main` would collide with the consumer's).
Copy from `docs/tinox-core/<module>/examples/` into the staged project's
`examples/` subdirectory before running `tinox doc` (same one-directory-
per-module location regardless of which version you're currently
generating docs for — examples aren't re-versioned per release, only
updated by hand if they go stale against a new version's actual API). As
of 2026-08-10 only the 13 modules bumped that day
(amqp091/amqp10/crypto/http/http2_server/http3_server/http_server/jwt/
oauth2/oidc/rest/websocket/zip) have this `docs/tinox-core/<module>/
examples/` directory restored (recovered by stripping the syntax-
highlighting markup back out of each module's previous docs.html, since
the original example sources were never committed as standalone files,
only their rendered HTML) — the other extended-tier modules' examples are
still only baked into their existing 1.0.0 docs.html with no editable
source anywhere; restore theirs the same way before their next version
bump, or that module's next docs.html will silently lose its Examples
section.

## File Structure: One Class/Interface/Enum per File

Since 2026-07-26 this is hard-enforced at the compiler level (a hard
compile error, not a lint/warning): **every `.tnx` file contains at
most ONE top-level `class`/`interface`/`enum` declaration**, and if it
contains one, the file name MUST exactly (case-sensitively) match the
type name (`class Player` → must be `Player.tnx`). Files with no type
at all (plain `fn`/`main` scripts, e.g. most `tests/e2e/*.tnx`) are
unaffected — the rule is "at most one", not "exactly one".

- **Modules with multiple types become directories.** `import
  tinox.core.amqp10;` (the namespace segment stays unchanged, e.g.
  still lowercase) now resolves to a directory
  `crates/tinox-core/amqp10/` that contains exactly one
  `<TypeName>.tnx` file per type (`Amqp10Connection.tnx`,
  `Amqp10Session.tnx`, …) — ONE `import` statement still pulls in every
  file in the directory, nothing changes for callers. This applies
  uniformly to both stdlib AND project-local imports (`import
  mymodule.foo;` works identically with a `foo/` directory instead of a
  `foo.tnx` file) — resolved in `resolve_imports()`
  (`crates/tinox/src/main.rs`): first `<name>.tnx` (legacy single-file
  case), otherwise `<name>/*.tnx` (all files in the directory merged).
- **Driver/entry-point files (with `main()` or `// expect:`
  directives) keep their name.** Their embedded types move into sibling
  files (flat in the same directory, or in a subdirectory
  `<original-name>/` if type names would collide with another file),
  the driver instead gets `import <TypeName>;` lines. This keeps
  `scripts/dogfood.sh` and e2e-harness paths stable (see the 2026-07-26
  migration example: `examples/vtable_dispatch.tnx` stayed the entry
  point, its three types moved to `examples/vtable_dispatch/*.tnx`).
- **Watch out for sibling imports within the same (sub)directory:
  ALWAYS use the short, unqualified name** (`import IDrawable;`),
  NEVER the full dotted path the OUTER driver uses (`import
  vtable_dispatch.IDrawable;`) — the full path is relative to the
  directory of the IMPORTING file, so from inside the directory itself
  it would look for a non-existent, doubly-nested subfolder level
  (`vtable_dispatch/vtable_dispatch/IDrawable.tnx`) and fail with "file
  not found".
- **Finding from the migration (2026-07-26, affected effectively every
  program with a `main()` that upcasts an imported class against an
  equally imported interface):** `resolve_imports()` appended imported
  declarations to the END of the decl list, but the typechecker only
  fills `interface_implementations` lazily during the sequential pass
  (`check_class` in `tinox-typecheck/src/lib.rs`) — if `main()` (from
  the driver file) came before the imported interface/class
  declarations, the implements table was still empty when checking
  `main()`'s body ("expected IDrawable, found Circle"). Fix: imported
  declarations are now placed BEFORE the importing file's own
  top-level declarations (`resolve_imports` collects them separately
  and prepends instead of appending). For any future rework of the
  import-merge logic: don't break this ordering invariant, or this
  exact pattern breaks again silently (a silent-garbage trap: compiles
  unchanged for single-file programs, only multi-file programs with an
  interface upcast are affected).

## Mandatory Entry Point: `class Main` + CDI-Style Bootstrap (since 2026-08-09)

Since 2026-08-09 this is hard-enforced at the compiler level
(`compile_file` in `crates/tinox/src/main.rs`, `has_class_named_main`):
**every program built via `tinox build`/`tinox run` needs `class Main {
fnc main() -> Int32 }`** in the entry file — otherwise a hard compile
error instead of the old, confusing "undefined reference to
tinox_main" linker error. Exempt are `@Command` CLI programs (their
own argv dispatch, their own generated `main`) and `tinox test` (its
own test-runner entry) — both unchanged. `tinox check` only checks
types and never invokes codegen, so it's unaffected too.

**Why:** previously, every auto-run annotation (`@Http3RestController`/
`@WebsocketEndpoint`/`@Amqp10Consumer`/`@Amqp091Consumer`/plain `@GET`/
`@Path`) generated its own `@tinox_main` — "whoever runs first wins"
(the `has_main` flag), and `class Main` ALWAYS won first (it ran first
in `gen()`), which meant other annotations in the same program were
silently NOT wired up — no error message, the routes simply never ran.
Now there's a single, uniformly structured bootstrap
(`emit_tinox_main_bootstrap` in `crates/tinox-codegen/src/codegen.rs`)
instead: it spawns every auto-run component found in the program on its
own real thread (`tinox_task_spawn`, the same mechanism `spawn` uses),
then calls `Main.main()`, and afterward joins every spawned thread
(blocks forever if any are running — exactly like a single, direct
`.listen()` call did before).

- **Cross-kind combinations are now allowed** (previously hard-blocked
  in `main.rs`): `@Http3RestController` + `@WebsocketEndpoint`/
  `@Amqp10Consumer`/`@Amqp091Consumer` in the same program, or any
  combination of those together with `class Main` — they no longer
  compete for the same `@tinox_main` symbol.
- **Since 2026-08-09 (phase 4), multiple instances of the SAME kind are
  also allowed** for `@WebsocketEndpoint`/`@Amqp10Consumer`/
  `@Amqp091Consumer` (not for `@Http3RestController` — it still routes
  ALL `@GET`/… in the program to a single server, multiple instances
  would be architecturally ambiguous, deliberately out of scope).
  `emit_ws_code`/`emit_amqp10_consumer_code`/
  `emit_amqp091_consumer_code` now iterate over every class found
  instead of hard-reading `[0]`, and generate a uniquely named
  `__tinox_run_<kind>_<idx>()` per instance. For `@WebsocketEndpoint`,
  `compile_file` additionally checks for duplicate ports (each one
  binds its own listening socket — two on the same port would
  otherwise be a silent bind failure only surfacing at runtime); for
  the two AMQP consumer kinds there is NO port-collision check, since
  multiple consumers against the same broker/port with different
  queues/addresses is the normal, expected case.
- **New concurrency trap that couldn't structurally exist before:**
  previously, only ONE auto-run kind ever ran per process, so a
  singleton shared via `@ApplicationComponent` was implicitly safe
  (only one event loop ever touched it). Now that, say, a REST
  controller AND a WebSocket endpoint can run at the same time on
  real, independent threads, a singleton field shared between the two
  is accessed genuinely concurrently for the first time. The compiler
  does NOT synchronize this automatically (disproportionately invasive
  for v1) — synchronize manually (`tinox.core.semaphore`) when sharing
  mutable state across component kinds.
- **Example migration (2026-08-09):** annotation-only files without
  their own `class Main` (`examples/rest_minimal`,
  `examples/rest_with_mini`) got a trivial `Main.tnx`; single-file
  demos sitting flat in `examples/` with no directory
  (`UserController.tnx`, `EchoEndpoint.tnx`, `DemoConsumer.tnx`,
  `DemoConsumer091.tnx`) each moved into their own directory with a
  `Main.tnx` (`examples/rest_auto/`, `examples/ws_echo_annotated/`,
  `examples/amqp10_consumer_annotated/`,
  `examples/amqp091_consumer_annotated/`). `examples/http3_rest_api/
  src/TaskController.tnx` couldn't get its own `Main.tnx` next to the
  existing imperative `src/Main.tnx` (name collision), so it moved
  into its own sibling example instead,
  `examples/http3_rest_api_annotated/`. `scripts/dogfood.sh` and the
  affected `crates/tinox/tests/*.rs` paths were updated accordingly.

## Runtime Quirks (not obvious from the code)

- **`spawn` starts a real POSIX thread** (`pthread_create` in
  `tinox_task_spawn`, runtime.c), not a compiled coroutine state
  machine — real parallelism, no cooperative scheduling.
- **The Boehm GC uses `SIGPWR` as its "stop the world" signal** on this
  system (verified via `gdb`, not the often-assumed `SIGRTMIN`). Every
  blocking syscall (`recv`/`send`/…) in runtime code that could run
  during a GC collision MUST retry on `EINTR` (already done this way in
  `conn_recv`/`conn_send` — the template for new blocking I/O code).
- **Debugging technique for hard-to-reproduce runtime bugs:**
  `coredumpctl` doesn't produce dumps in this environment (sandbox
  restriction). `gdb` with conditional breakpoints on hot paths (e.g.
  `tinox_array_get`, called on every byte access) is unusably slow;
  `gdb` also needs `handle SIGPWR nostop noprint pass`, otherwise it
  keeps stopping on the harmless GC-suspend signal. Instead: add a
  temporary `errno` debug print, or a minimal `signal(SIGSEGV,
  handler)` with `backtrace()`/`backtrace_symbols_fd()` in `runtime.c`,
  then resolve the raw `[0x...]` addresses from the log with
  `addr2line -f -C -e <binary> <address>`. Remove again after
  debugging.

## `tinox docker`: Minimal Docker Images from a Project (since 2026-08-11)

`tinox docker` (`crates/tinox/src/main.rs`, `docker_build`) compiles the
project (same pipeline as `tinox build`, Release by default) and packages
the resulting binary into a minimal, single-stage Docker image: install
only the runtime shared libraries actually linked, `COPY` the binary in,
`EXPOSE` the configured ports, run it as the entrypoint. Config lives in
a `[docker]` section in `tinox.toml`:

```toml
[docker]
ports = [8080, 9090]        # optional, EXPOSE only -- doesn't change how
                             # the program binds them (still HttpServer::new(port) etc.)
image = "myapp"              # optional, defaults to [package].name
base = "debian:trixie-slim"  # optional, defaults shown; must be apt-based (see below)
extra_packages = ["libpq5"]  # optional, appended to the auto-detected apt package list
```

`--tag name:tag` overrides the image name+tag outright (from either
`tinox.toml` or the derived default); `--debug` compiles Debug instead of
Release.

- **The compiled binary is copied in from the host, not rebuilt inside the
  container.** A full multi-stage build (matching-glibc builder image,
  Rust+LLVM+clang toolchain, vendoring the compiler source into the build
  context) would remove the glibc-compatibility caveat below entirely, but
  is disproportionately invasive for what was asked for — a lightweight,
  minimalistic mechanism. Documented limitation, not a bug: `[docker] base`
  needs a glibc new enough for the host-compiled binary (older host glibc
  than the image's is fine; a newer host glibc generally is not). Default
  is `debian:trixie-slim` (glibc 2.41, current Debian stable as of
  2026-08) rather than `bookworm-slim` (glibc 2.36) -- the older default
  tripped the `ldd` check below on the very first two real-world runs
  (this dev machine, Arch glibc 2.44, and a user's machine needing
  `GLIBC_2.38`), so it wasn't just a theoretical edge case.
- **This is exactly the kind of thing the project's "no silent garbage"
  philosophy exists for, so it isn't just documented — it's enforced at
  build time:** after `docker build`, `docker_build` runs `ldd` on the
  copied-in binary inside the freshly built image and greps for "not
  found". Any missing symbol/library hard-fails the command with the exact
  `ldd` line instead of silently tagging a broken image as built. Verified
  live on this dev machine (Arch, glibc 2.44): `debian:bookworm-slim`
  (glibc 2.36) correctly hard-failed with `GLIBC_2.38 not found`;
  switching `[docker] base` to `debian:trixie-slim` (2.41) then built,
  passed the `ldd` check, and `docker run`'s output was verified against
  `curl` end-to-end (a standalone annotation-based REST demo project, not
  part of this repo). This dev machine's
  glibc (2.44) is itself still ahead of every current apt-based image
  including `debian:sid-slim` (2.42) and `ubuntu:devel` (2.43) -- expect
  `tinox docker`'s default to occasionally need a newer `base` override on
  bleeding-edge rolling-release hosts even after this bump.
- **Only apt-based (Debian/Ubuntu-family) base images are supported.** The
  generated Dockerfile's package-install step is hardcoded to
  `apt-get` — `[docker] base` pointing at an Alpine/Arch/etc. image will
  fail at that `RUN` step, not silently produce a broken image, but it
  won't work either. Not handled: scope was "minimal apt-based runtime
  image", not multi-package-manager support.
- **Package selection mirrors `compile_ll_to_exe`'s own link flags exactly**
  (`docker_runtime_packages`/`compute_runtime_packages`), rather than a
  fixed guess: `libgc1`+`zlib1g` always (matches unconditional `-lgc -lz`),
  `libssl3` when TLS is on (default on, matches `-lssl -lcrypto`, opt-out
  via `TINOX_TLS=0` same as the compiler), `libpq5`/`libmariadb3`/
  `libsqlite3-0` from `[database] driver` when set. `TINOX_HTTP3=1` prints
  a warning instead of guessing ngtcp2/nghttp3 package names (they vary by
  distro) — add them via `extra_packages` if needed; the `ldd` check
  catches it either way if they're missing.

## Startup Banner for Auto-Run Programs (since 2026-08-11)

Every compiled program that has at least one auto-run endpoint (`@GET`/
`@Http3RestController`/`@WebsocketEndpoint`/`@Amqp10Consumer`/
`@Amqp091Consumer`) prints a startup banner by default — no `import
tinox.core.logger;` or annotation needed. Owned by
`emit_tinox_main_bootstrap` (`crates/tinox-codegen/src/codegen.rs`),
since that's already the one place that knows about every registered
auto-run kind and is guaranteed to run exactly once, first:

```
 _____ _
|_   _(_)_ __   _____  __
  | | | | '_ \ / _ \ \/ /
  | | | | | | | (_) >  <
  |_| |_|_| |_|\___/_/\_\
Loaded tinox.core modules: http_server, json
Endpoints:
  HTTP                   :8080
Started in 0 ms
```

- **Only fires when `background_run_fns` is non-empty AND `banner_enabled`
  is true** (`show_banner` in `emit_tinox_main_bootstrap`). A plain
  `class Main { fnc main() }` script with no auto-run annotation goes
  through the *same* function (`user_main_class` alone doesn't early-
  return) but must produce byte-identical output to before this feature
  — that's the shape virtually every e2e/example test with an exact `//
  expect:` stdout match uses. **Verify the `background_run_fns`-empty
  half of this gate whenever touching this function**: the very first
  implementation forgot it, and every single compiled program (including
  the entire e2e suite) grew this banner — caught immediately by
  compiling a trivial one-`println` `class Main` and diffing its output,
  not by `cargo test` (no e2e test happens to combine an auto-run
  annotation with an exact stdout match, so the suite itself wouldn't
  have caught this).
- **Explicit per-project opt-out: `[startup]` / `banner = false` in
  tinox.toml** (`read_startup_banner_config` in `crates/tinox/src/
  main.rs`, defaults `true`; `CodeGen::banner_enabled` /
  `set_startup_banner_enabled`). Added because jgrep-tinox/ygrep-tinox
  are plain argv-parsing CLI tools with no auto-run endpoint, so
  `background_run_fns` is already empty for them and the banner never
  fires regardless — this setting only matters for a program that DOES
  have an endpoint (so the banner would otherwise print) but still needs
  clean stdout, e.g. piped into another program.
- **"Loaded tinox.core modules"** is `tinox.toml`'s declared
  `[[dependencies]]` filtered to `group == "tinox.core"`
  (`loaded_tinox_core_modules` in `crates/tinox/src/main.rs`, read
  alongside `load_dep_dirs` in `compile_file` and passed to codegen via
  `CodeGen::set_loaded_modules`) — declared, not actually-imported.
  Simpler, and accurate enough: an unused declared dependency is already
  the unusual case, not the common one this needs to optimize for.
- **"Endpoints:"** is `(protocol, detail)` pairs pushed into
  `CodeGen::startup_endpoints` right alongside each `background_run_fns`
  push (same emit_*_code functions, so always in sync): `("HTTP",
  ":8080")`, `("HTTP/3 (QUIC)", ":8843")`, `("WebSocket", ":9001")`,
  `("AMQP 0-9-1 (consumer)", "host:port (queue: q)")`, `("AMQP 1.0
  (consumer)", "host:port (address)")`. AMQP consumers connect out
  rather than bind a port, hence the different (no leading `:`) shape.
- **"Started in N ms"** is wall-clock from the top of `@tinox_main`
  (before the banner print) to right after every auto-run kind has been
  `tinox_task_spawn`-ed (before calling `Main_main`) — via
  `tinox_now_ms()` (runtime.c, `clock_gettime(CLOCK_MONOTONIC, ...)`),
  diffed on the Tinox side (two IR-level calls + a `sub`, no runtime
  elapsed-time helper needed). This is "time to bring up the bootstrap",
  not "time until the first successful request" — `HttpServer::listen()`s
  actual bind happens asynchronously on its own spawned thread, so a
  slow/failing bind is invisible to this number, same tradeoff Spring
  Boot's own "Started Application in Xs" line makes.

## Dev UI Introspection API (since 2026-08-11)

`[dev] enabled = true` in `tinox.toml` (`DevConfig`/`read_dev_config`,
`crates/tinox/src/main.rs`) compiles in a background JSON introspection API
(`emit_devui_code`, `crates/tinox-codegen/src/codegen.rs`) for a *separate*
web dashboard (`tinox-devui`, a standalone Vaadin-on-Quarkus app,
`git@github.com:subnix-work/tinox-devui.git`, not part of this repo) to
consume — Quarkus-dev-mode-style: current config, REST/
WebSocket endpoints, live CDI component status, loaded `tinox.core`
modules. Enabling it works for `tinox build`/`run` too, not gated behind
`tinox dev` specifically (deliberate — `compile_file` prints a release-
build warning as the safety net instead of a hard gate).

- **`127.0.0.1`-only bind, unlike every other `HttpServer` in this
  codebase.** New runtime.c primitive `tinox_HttpServer_new_bind(port,
  addr)` (the public `HttpServer::new(port)` stays `0.0.0.0`/`::`
  unchanged) — this API exposes config and CDI internals, so it must never
  be reachable off the local machine. Verified live: `ss -ltnp` on a
  devui-enabled `demo` run shows the app's own port on `0.0.0.0`, the devui
  port on `127.0.0.1` only.
- **Found and fixed a real concurrency bug while adding this**: adding a
  *second* `HttpServer::listen()` call in the same process (previously
  impossible — before this feature, no program ever ran more than one) hit
  `struct TinoxWorkerArgs { ... }` being `static` in
  `tinox_HttpServer_listen` (runtime.c) — shared storage across every call
  to that function, not per-instance. With two listening servers, the
  second's worker args silently clobber the first's while its still-
  running worker threads keep reading it, serving the wrong server's
  routes on the wrong port. Fixed by making it a plain stack local
  (`tinox_HttpServer_listen` never returns while its server is up, so the
  stack frame outlives the spawned workers, same as `static` did — just
  scoped per-call instead of shared).
- **Found and fixed a real, unrelated pre-existing bug while studying this
  pattern**: the `/metrics` endpoint's `Content-Type` header
  (`emit_route_code`'s metrics shim) computed `%ct_hdr_val` and then never
  used it — `%body_i64` (the response body's own pointer) was passed as
  the header *value* instead, so every `/metrics` response's Content-Type
  header ended up set to the same string as its body. No test ever
  exercised the header specifically, so nothing caught it until this
  investigation.
- **`declare`-conflict landmine for anything emitted alongside
  `emit_route_code`**: `opt` hard-errors ("invalid redefinition") on a
  *second* `declare` for a symbol already declared elsewhere in the
  module, even with an identical signature -- contrary to what an earlier
  draft of this feature assumed from an unverified reading of the existing
  double-`declare` of `tinox_HttpServer_new` inside the metrics shim
  (which, it turns out, never actually co-occurs with `emit_route_code`'s
  own copy in any tested program — a genuinely separate, still-open,
  latent bug: a program with **both** `[metrics]` enabled **and** real
  `@GET`/etc. routes would hit the exact same "invalid redefinition"
  class of error the devui work below had to route around; not fixed
  here, out of scope for this feature). `emit_devui_code` mirrors
  `emit_route_code`'s own `route_entries.is_empty() ||
  http3_rest_controller.is_some()` guard to decide whether it's safe to
  declare `tinox_HttpServer_get`/`_listen` itself.
- **`/components`** (`emit_devui_components_handler`) is the one endpoint
  needing real per-request work: `@ApplicationComponent`/`@Startup`-scoped
  classes get a live look at their `@{class}_di_instance` global (null or
  not); `@HttpRequestScoped` ones report a constant `false` — they have no
  persistent singleton at all (`_di_create()` allocates fresh every call,
  never caches), so there's nothing to check.
- **`/config`** merges two genuinely separate sources at runtime via
  `tinox_string_concat`: a compile-time summary of `tinox.toml`'s
  `[docker]`/`[database]`/`[metrics]`/`[startup]` sections
  (`build_dev_config_summary_json`, main.rs — deliberately omits
  `[database] url`, which can carry credentials, even though this
  endpoint is loopback-only) and a live dump of `application.properties`
  (`tinox_config_dump_json`, new in runtime.c — the existing
  `tinox_config_get*` only ever look up one key a `@Config` field already
  declared, there was no "list everything" API before this).
- **`httpPort` on `/info`**: the app's plain-HTTP port (`self.startup_
  endpoints`'s `"HTTP"` entry, already registered by `emit_route_code`,
  which runs before `emit_devui_code`), `null` for an HTTP/3-only program.
  This is what `tinox-devui`'s REST "try it out" targets — deliberately
  NOT the introspection port itself, and NOT `"HTTP/3 (QUIC)"` (a plain
  `java.net.http.HttpClient` can't speak QUIC; HTTP/3-only apps just don't
  get try-it-out in v1).

## `tinox-devui` Dashboard + `tinox dev` Docker Orchestration (since 2026-08-12)

The consumer side of the introspection API above: a standalone Maven/
Quarkus/Vaadin app (`tinox-devui` repo, dark Lumo theme matching
tinox-central's `registry-frontend`) with an `AppLayout`+`SideNav` shell
and one view per introspection endpoint (Overview/Configuration/REST
Endpoints/WebSocket Endpoints/CDI Components/Modules). `TinoxDevUiClient`
(`@ApplicationScoped`, plain `java.net.http.HttpClient` + manual Jackson,
mirrors tinox-central's `RegistryClient.java` pattern) talks to the
connected app's `[dev] port` (`tinox.app.url` / `TINOX_APP_URL`,
default `http://localhost:9090`).

- **REST "try it out"** (`RestEndpointsView`): a dialog per route with a
  `TextField` per `:param` path segment (parsed via regex, substituted
  and URL-encoded before the call), a raw headers textarea (`"Name:
  value"` per line), a body textarea, and a "Send" button that calls
  `TinoxDevUiClient.invoke(httpPort, method, path, headers, body)` --
  server-side, against the app's OWN `httpPort` (from `/info`), never the
  introspection port and never directly from the browser. This is why
  there's no CORS story on the tinox side (decision made during planning,
  see the approved plan) -- the browser only ever talks to this Quarkus
  backend, which does the real HTTP call itself.
- **WebSocket "try it out"** (`WebSocketEndpointsView` + `DevUiWsClient`):
  same server-side-proxy shape, but for a persistent connection instead of
  one-shot calls. `DevUiWsClient` is a plain Jakarta WebSocket
  (`quarkus-websockets-client`) `Endpoint` connecting to the app's own WS
  port (`/websockets`' `port`, NOT the introspection port). Incoming
  messages arrive on the WS client's own thread, not Vaadin's request
  thread, so every UI update (transcript line, connected/disconnected
  status pill) goes through `UI.access(...)` -- requires `@Push` on
  `AppShellConfig`, the one piece of Vaadin server-push wiring this whole
  app needs, added specifically for this view.
- **`tinox dev` orchestration** (`launch_devui_container`/
  `stop_devui_container`, `crates/tinox/src/main.rs`): when the project's
  `[dev]` is `enabled`, `tinox dev` additionally `docker run -d --rm
  --network host` the `tinox-devui` image (tag from `[dev] devui_image`,
  default `tinox-devui:latest` -- a locally built image; override once a
  real registry tag exists) alongside the compiled program, with
  `TINOX_APP_URL` pointed at `127.0.0.1:<dev.port>`, then opens
  `http://localhost:9091` the same way `tinox doc --open` already does
  (`xdg-open`/`open` fallback). `--network host` is what lets the
  container reach the loopback-only introspection API directly -- no
  `host.docker.internal`, Linux-only, matches this whole toolchain's
  target. A missing/unbuildable image is a soft failure (a printed
  warning, `tinox dev` still runs the actual program fine) rather than a
  hard error, consistent with `[dev]` itself being an opt-in convenience
  feature, not a build-blocking dependency.
- **Found a real cleanup gap while wiring this up, not hypothetical:**
  `dev_mode`'s only exit path before this was the file-watcher channel
  closing (`rx.recv()` returning `Err`) -- which a plain Ctrl-C never
  triggers. The compiled child process happened to look cleaned-up anyway
  (the terminal's own SIGINT delivery kills it directly, since it's in the
  same foreground process group), which is presumably why this was never
  noticed before. A `docker run` container is NOT in that process group
  though, so every single ordinary `tinox dev` + Ctrl-C session would have
  silently leaked a running `tinox-devui` container -- verified live: with
  no signal handler, `kill -INT` on `tinox dev`'s pid terminated the
  process immediately without running any Rust cleanup code, leaving the
  container in `docker ps`. Fixed by adding a real `ctrlc::set_handler`
  (new `ctrlc` dependency) sharing `Arc<Mutex<...>>`-wrapped child-process
  and container-name state with the main loop, calling the same cleanup
  closure (`kill` child, remove temp exe files, `docker stop` the
  container) from both the normal loop-exit path and the signal handler.
  Re-verified live after the fix: `kill -INT` now cleanly stops and
  removes (`--rm`) the container and leaves no leftover `.tinox_dev_*`
  files.
- **Published to `ghcr.io/subnix-work/tinox-devui` (since the `tinox-devui`
  repo's `v1.0.0` tag, 2026-08-12).** The image was `docker build`+`docker
  run`-verified locally first (against a real `demo`-style app, and
  end-to-end through `tinox dev` itself) before the registry push, per the
  plan's "publish only after manual validation" note.
  `.github/workflows/publish.yml` (in the `tinox-devui` repo) builds and
  pushes on every `vX.Y.Z` tag, using the repo's own `GITHUB_TOKEN`
  (`packages: write` permission -- no separate PAT/secret needed) via
  `docker/login-action`. `launch_devui_container`'s default `[dev]
  devui_image` is now this published tag (`ghcr.io/subnix-work/
  tinox-devui:latest`) rather than a locally-built-only `tinox-devui:latest`
  -- `docker run` pulls it automatically on a machine that's never built
  the dashboard itself. Override to a local build via `[dev] devui_image`
  in `tinox.toml` when developing the dashboard.
