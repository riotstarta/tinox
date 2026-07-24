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
geschlossen, Label, Volltext), nicht in einer lokalen Datei. Alte
Bug-Nummern aus der Zeit vor der Migration entsprechen exakt der
gleichnamigen Issue-Nummer (Bug 40 → Issue #40).
