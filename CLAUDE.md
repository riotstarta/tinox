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
Note the module's own `examples/*.tnx` (for the page's Examples section)
no longer exists anywhere on disk for extended-tier modules post-split —
that section will legitimately be absent from freshly generated pages
until/unless that content is restored somewhere discoverable.

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
