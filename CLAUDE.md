# Projektkonventionen für Claude Code

## Bug-/Feature-Tracking läuft über GitHub Issues

Seit 2026-07-25 werden **alle** Bugs und abgeschlossenen Feature-
Implementierungen als [GitHub Issues](https://github.com/subnix-work/tinox/issues)
auf `subnix-work/tinox` erfasst — nicht mehr in lokalen Markdown-Dateien
(`bugs.md`/`bugs_fixed.md` wurden entfernt, ihr kompletter Inhalt liegt
1:1 in den Issues #1–74).

**Verbindliche Regel für jeden neuen Fund/Fix ab jetzt:**

- **Neuer Bug gefunden** (egal ob sofort gefixt oder nicht): direkt ein
  GitHub Issue anlegen (`gh issue create --repo subnix-work/tinox`).
  Titel im Stil `Bug NN — Kurzbeschreibung` (fortlaufende Nummer, an die
  letzte vergebene Issue-Nummer anschließen) oder `Feature: Name` für
  abgeschlossene Feature-Arbeit. Label `bug` bzw. `enhancement`.
- **Bug ist gefixt:** Issue-Body enthält (wie bisher in bugs.md/
  bugs_fixed.md üblich) Status/Root-Cause/Fix/Verifiziert — dann Issue
  schließen (`gh issue close <NR> --reason completed`).
- **Bug ist noch offen** (bewusst zurückgestellt oder ungelöst): Issue
  bleibt offen, Body beschreibt den Stand + warum er offen ist.
- **Sprache: Englisch** (Titel + Body) — die Issues wurden bewusst ins
  Englische übersetzt und sollen es bleiben (seit 2026-07-26 gilt das
  ohnehin projektweit auch für Commit-Messages und Code, s. u.).
- Cross-Referenzen zwischen verwandten Issues wie bisher zwischen
  Bug-Einträgen üblich (z. B. „closes what Bug 40 left open" mit Link
  auf die Issue-Nummer).

**Historie nachschlagen:** in den GitHub Issues suchen/filtern (offen vs.
geschlossen, Label, Volltext), nicht in einer lokalen Datei. **Achtung:**
die alte „Bug NN"-Nummer aus der Zeit vor der Migration entspricht
**NICHT** zuverlässig derselben Issue-Nummer (z. B. ist „Bug 40"
tatsächlich Issue #41 — eine dazwischenliegende Notiz-Überschrift ohne
Bug-Nummer hat die Zählung verschoben, und mehrere Bugs wurden teils zu
einem einzigen Issue zusammengefasst, z. B. „Bugs 64–65" inkl. der darin
eingebetteten Bugs 66–71 als EIN Issue). Immer per Titel suchen
(`gh issue list --repo subnix-work/tinox --state all --search "Bug 40"`),
nicht per angenommener Nummer.

## Kernphilosophie (aus 70+ dokumentierten Bugs destilliert)

- **Kein Silent-Garbage.** Jeder Fehlerfall bekommt einen harten,
  sichtbaren Fehler statt stiller Datenkorruption oder eines leisen
  Default-Werts. Im Zweifel: hart abbrechen mit klarer Meldung statt
  „funktioniert meistens schon irgendwie". Das ist die mit Abstand
  häufigste Root-Cause-Kategorie im gesamten Bug-Log.
- **Gegen echte, unabhängige Systeme verifizieren, nicht nur
  selbstkonsistent testen.** Simulierte Broker/Server-Tests (via
  `spawn`/`await`) sind nötig und gut, finden aber strukturell KEINE
  Bugs, bei denen die eigene Implementierung mit sich selbst konsistent,
  aber falsch ist (z. B. Bug 70/71: `initial-delivery-count`-Pflichtfeld
  und `amqp-value`-vs-`data`-Kodierung wurden nur durch Live-Tests gegen
  echtes RabbitMQ bzw. einen unabhängigen Python-Client gefunden). Bei
  Netzwerk-/Protokoll-Features: wenn irgend möglich, zusätzlich gegen
  eine echte, fremde Implementierung verifizieren.
- **Gezielt statt pauschal fixen.** Bei einem gefundenen Bug den
  kleinstmöglichen, gut abgegrenzten Fix wählen statt einen größeren,
  riskanteren Umbau zu erzwingen — auch wenn der „saubere" Umbau
  theoretisch reizvoll wäre. Bekannte, dokumentierte Design-Grenzen
  (offene Issues) sind ein akzeptables Ergebnis, wenn der vollständige
  Fix unverhältnismäßig invasiv wäre.
- **Bevor ein „offener" Punkt angegangen wird: prüfen, ob nicht schon
  ein SPÄTERER Fix ihn geschlossen hat.** Mehrfach in der Historie stand
  „noch offen" in einem Eintrag, der bereits durch den direkt folgenden
  Eintrag im selben Log erledigt war (z. B. Bug 35s Restschwäche →
  gefixt in Bug 40; ein `.toString()`-Fund bei Bug 38 → gefixt in Bug
  39). Erst per Repro nachstellen, dann Zeit investieren.

## Build & Test

- `make check` (clippy + Unit-Tests + e2e/matrix/boundary/stdlib_smoke
  doppelt + Dogfood inkl. `jgrep-tinox`) muss vor jedem Commit komplett
  grün sein — dauert 15–25 Minuten. Im Hintergrund laufen lassen
  (`nohup ... & disown`, Log-Datei pollen), nicht blockierend abwarten.
  Ein Fehlschlag ist erstmal eine ECHTE Regression, keine angenommene
  Flakiness — aber: reine Bind-/Port-Fehler in E2E-Tests können durch
  Port-Kollisionen zwischen zwei Testdateien entstehen (schon einmal
  passiert), das lohnt einen kurzen `grep -rn "httpServerCreate(4" tests/`
  Check, bevor man es als „nur flaky" abtut.
- `make asan` (AddressSanitizer, `-DTINOX_NO_GC`) und `make checked`
  (Heap-Kind-Registry, `-DTINOX_CHECKED`) sind NICHT Teil von
  `make check`, aber sinnvoll bei Verdacht auf Speicherfehler/Dispatch-
  Bugs auf falschem Heap-Objekt-Typ — laut Makefile-Kommentar für
  wöchentliche/Vor-Release-Läufe gedacht.
- Neue e2e-Tests unter `tests/e2e/*.tnx` mit `// expect:`-Direktiven
  (Zeile-für-Zeile-Abgleich der stdout-Ausgabe). Bei Tests, die einen
  Port binden: einen tatsächlich freien Port wählen (`grep -rn
  "httpServerCreate(4" tests/e2e/*.tnx examples/*.tnx` zeigt belegte).
- Tests, die `spawn`/`await` nutzen (simulierter Broker/Server via
  Loopback), 15–40× stabil wiederholen, bevor sie als grün gelten — die
  Async-Runtime hatte mehrere zeitabhängige Bugs (Bug 68 u. a.), die nur
  bei wiederholten Läufen auffielen.
- **Seit 2026-07-26: Commit-Messages UND Code (inkl. Kommentare, Bezeichner,
  Doc-Strings) sind auf Englisch** — sowohl in diesem Repo als auch in
  Downstream-Projekten wie jgrep-tinox. Ältere Commits/Kommentare bleiben
  auf Deutsch (nicht rückwirkend ändern, nur neue Arbeit betrifft das).
  Vorherige Konvention (Commit-Messages auf Deutsch im Stil der alten
  bugs.md-Einträge: Root Cause, Fix, Verifiziert) ist damit abgelöst —
  Struktur/Inhalt der Commit-Message bleibt gleich, nur die Sprache wechselt.
- **`docs.html` (Deutsch) und `docs_en.html` (Englisch) sind bewusst
  parallel gepflegte Dubletten** — bei jeder neuen `<div class="mod-
  section">` in `docs.html` (neues Stdlib-Modul) IMMER auch
  `docs_en.html` ergänzen (Nav-Link, Übersichts-Karte falls vorhanden,
  Modul-Sektion übersetzt). War schon einmal seit Mai wochenlang out of
  sync (WebSocket/AMQP-091/AMQP-1.0 fehlten in der EN-Version bis
  2026-07-25) — nicht wieder passieren lassen. Quick-Check bei Zweifel:
  `grep -oE 'id="mod-[a-z0-9_]+"' docs.html | sort -u` gegen dieselbe
  Zeile für `docs_en.html` diffen, muss leer sein.

## Dateistruktur: eine Klasse/Interface/Enum pro Datei

Seit 2026-07-26 gilt hart compilerseitig erzwungen (harter Compile-Fehler,
kein Lint/Warning): **jede `.tnx`-Datei enthält höchstens EINE
Top-Level-`class`/`interface`/`enum`-Deklaration**, und falls sie eine
enthält, MUSS der Dateiname exakt (case-sensitive) dem Typnamen entsprechen
(`class Player` → zwingend `Player.tnx`). Dateien ganz ohne Typ (reine
`fn`/`main`-Skripte, z. B. die meisten `tests/e2e/*.tnx`) sind davon
unberührt — die Regel ist „höchstens eine", nicht „genau eine".

- **Module mit mehreren Typen werden zu Verzeichnissen.** `import
  tinox.core.amqp10;` (Namespace-Segment bleibt unverändert, z. B.
  weiterhin klein geschrieben) löst jetzt auf ein Verzeichnis
  `crates/tinox-core/amqp10/` auf, das pro Typ genau eine
  `<TypeName>.tnx`-Datei enthält (`Amqp10Connection.tnx`,
  `Amqp10Session.tnx`, …) — EIN `import`-Statement zieht weiterhin alle
  Dateien im Verzeichnis rein, für Aufrufer ändert sich nichts. Das gilt
  einheitlich für Stdlib- UND projektlokale Imports (`import
  mymodule.foo;` funktioniert identisch mit einem `foo/`-Verzeichnis statt
  einer `foo.tnx`-Datei) — resolution in `resolve_imports()`
  (`crates/tinox/src/main.rs`): erst `<name>.tnx` (Legacy-Einzeldatei-Fall),
  sonst `<name>/*.tnx` (alle Dateien im Verzeichnis gemergt).
- **Treiber-/Entry-Point-Dateien (mit `main()` oder `// expect:`-
  Direktiven) behalten ihren Namen.** Ihre eingebetteten Typen wandern in
  Geschwister-Dateien (flach im selben Verzeichnis oder in einem
  Unterverzeichnis `<original-name>/`, falls Typnamen mit einer anderen
  Datei kollidieren würden), der Treiber bekommt stattdessen
  `import <TypeName>;`-Zeilen. So bleiben `scripts/dogfood.sh`- und
  e2e-Harness-Pfade stabil (siehe Migration examples 2026-07-26:
  `examples/vtable_dispatch.tnx` blieb Entry-Point, seine drei Typen
  wanderten nach `examples/vtable_dispatch/*.tnx`).
- **Achtung Geschwister-Imports innerhalb desselben (Unter-)Verzeichnisses:
  IMMER der kurze, unqualifizierte Name** (`import IDrawable;`), NIEMALS
  der volle gepunktete Pfad wie ihn der AUSSENSTEHENDE Treiber benutzt
  (`import vtable_dispatch.IDrawable;`) — der volle Pfad ist relativ zum
  Verzeichnis der IMPORTIERENDEN Datei, würde also aus dem Verzeichnis
  selbst heraus eine nicht existierende doppelt verschachtelte
  Unterordnerebene suchen (`vtable_dispatch/vtable_dispatch/IDrawable.tnx`)
  und mit „file not found" fehlschlagen.
- **Fund bei der Migration (2026-07-26, betraf faktisch jedes Programm mit
  `main()`, das eine importierte Klasse gegen ein ebenfalls importiertes
  Interface hochcastet):** `resolve_imports()` hängte importierte
  Deklarationen ans ENDE der Decl-Liste an, aber der Typechecker füllt
  `interface_implementations` erst lazy beim sequenziellen Durchlauf
  (`check_class` in `tinox-typecheck/src/lib.rs`) — stand `main()` (aus der
  Treiber-Datei) vor den importierten Interface-/Klassen-Deklarationen,
  war die Implements-Tabelle beim Prüfen von `main()`s Body noch leer
  („expected IDrawable, found Circle"). Fix: importierte Deklarationen
  werden jetzt VOR die eigenen Top-Level-Deklarationen der importierenden
  Datei gestellt (`resolve_imports` sammelt sie separat und prependt statt
  zu appenden). Bei jedem künftigen Umbau der Import-Merge-Logik: dieses
  Ordering-Invariant nicht brechen, sonst bricht genau dieses Muster
  wieder lautlos (Silent-Garbage-Falle: kompiliert bei Single-File-
  Programmen unverändert weiter, nur Mehrdatei-Programme mit
  Interface-Upcast sind betroffen).

## Runtime-Eigenheiten (nicht offensichtlich aus dem Code)

- **`spawn` startet einen echten POSIX-Thread** (`pthread_create` in
  `tinox_task_spawn`, runtime.c), keine kompilierte Coroutine-State-
  Machine — echte Parallelität, kein kooperatives Scheduling.
- **Der Boehm-GC nutzt `SIGPWR` als „Stop the world"-Signal** auf diesem
  System (per `gdb` verifiziert, nicht das oft angenommene `SIGRTMIN`).
  Jeder blockierende Syscall (`recv`/`send`/…) in Runtime-Code, der
  während einer GC-Kollision laufen könnte, MUSS auf `EINTR` mit Retry
  reagieren (bereits so in `conn_recv`/`conn_send` — Vorbild für neuen
  blockierenden I/O-Code).
- **Debugging-Technik für schwer reproduzierbare Runtime-Bugs:**
  `coredumpctl` liefert in dieser Umgebung keine Dumps (Sandbox-
  Restriktion). `gdb` mit conditional breakpoints auf heiße Pfade
  (z. B. `tinox_array_get`, wird pro Byte-Zugriff aufgerufen) ist
  unbrauchbar langsam; `gdb` muss außerdem `handle SIGPWR nostop
  noprint pass` bekommen, sonst stoppt es ständig am harmlosen
  GC-Suspend-Signal. Stattdessen: temporären `errno`-Debug-Print bzw.
  einen minimalen `signal(SIGSEGV, handler)` mit `backtrace()`/
  `backtrace_symbols_fd()` in `runtime.c` einbauen, dann die rohen
  `[0x...]`-Adressen aus dem Log mit `addr2line -f -C -e <binary>
  <adresse>` auflösen. Nach dem Debuggen wieder entfernen.
