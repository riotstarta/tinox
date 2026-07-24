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
- **Sprache: Englisch** (Titel + Body) — auch wenn Commit-Messages und
  sonstige Doku im Projekt auf Deutsch bleiben, die Issues wurden bewusst
  ins Englische übersetzt und sollen es bleiben.
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
- Commit-Messages in diesem Projekt sind auf Deutsch (technische Prosa
  im Stil der alten bugs.md-Einträge: Root Cause, Fix, Verifiziert),
  auch wenn Issues auf Englisch sind — beides bewusst so.

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
