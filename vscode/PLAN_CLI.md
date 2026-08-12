# PLAN_CLI.md — VS Code Extension für jgrep-tinox (Ansatz A)

**Ansatz A:** Die Extension ruft das lokal installierte CLI-Tool [jgrep-tinox](https://github.com/subnix-work/jgrep-tinox) als Child-Prozess auf. Sie bringt die jq-Engine nicht selbst mit und kompiliert Tinox nicht.

**Ort:** analog zur bestehenden Eclipse-Integration unter `tinox/eclipse/` entsteht die VS-Code-Extension unter **`tinox/vscode/`**. Dieses Dokument liegt bereits in diesem Ordner.

**Dieser Plan beschreibt die spätere Umsetzung. Er ist keine Implementierung.** Es wird hier kein Extension-Code erzeugt und kein Scaffolding ausgeführt.

---

## 0. Ausgangslage

### 0.1 Was jgrep kann (für den Wrapper relevant)

Aus README und CLI-Quellen von jgrep-tinox (`Main.tnx`, `ArgParser.tnx`, `CliOptions.tnx`):

| Thema | Verhalten |
| --- | --- |
| Aufruf | `jgrep [OPTIONS] FILTER [FILE…]` |
| Stdin | Ohne Dateiargument liest jgrep Stdin. |
| Keine Args | Hilfe auf stdout, Exit-Code 2. Die Extension muss immer mindestens den Filter übergeben. |
| Default-Filter | `.` (Identität), wenn der Parser nichts anderes setzt. Trotzdem explizit übergeben. |
| Binary-Name | `jgrep`, nicht `jgrep-tinox`. Dieselbe Binary als `ygrep` oder mit `--yaml` / `--json`. |
| Farbe | `--no-color` bzw. `--color never` — in VS Code keine ANSI-Codes. |
| Pretty | `--pretty` für lesbare JSON-Ausgabe. |
| Streaming | `-u` / `--unbuffered` nur für unbegrenzte Live-Streams. Für einen abgeschlossenen Editorpuffer unnötig: wir schreiben den Text und schließen Stdin. |

Beispiele, die der Wrapper nachbilden soll:

```bash
echo '{"name":"Alice"}' | jgrep '.name'
printf 'name: Alice\n' | ygrep '.name'
# bzw. explizit:
… | jgrep --json '.name'
… | jgrep --yaml '.name'
```

### 0.2 Warum Ansatz A

- Eine Engine, ein Verhalten: Filter, Exit-Codes und Fehlertexte bleiben identisch zur CLI.
- Ungespeicherte Puffer funktionieren, weil wir **nicht die Datei auf Disk** übergeben, sondern den Editortext über Stdin.
- Nutzer können den Binary-Pfad selbst setzen (Dev-Build, nicht-PATH-Installation).

### 0.3 Anlehnung an `tinox/eclipse/`

Die Tinox-Eclipse-Integration lebt bereits als Geschwisterordner:

```
tinox/eclipse/
├── README.md
├── SETUP.md
├── install-lsp.sh
└── tinox-eclipse/          ← eigentliches PDE-Plugin (plugin.xml, src/)
    ├── META-INF/MANIFEST.MF
    ├── plugin.xml
    └── src/tinox/eclipse/
```

Eclipse braucht die Extra-Ebene `tinox-eclipse/`, weil PDE ein OSGi-Bundle mit `META-INF/` und `plugin.xml` als Projektwurzel erwartet. Die Begleitdokumente liegen eine Ebene darüber.

VS Code braucht das nicht: die Projektwurzel **ist** der Ordner mit `package.json`. Deshalb ist `tinox/vscode/` selbst die Extension-Wurzel — funktional das Gegenstück zu `tinox/eclipse/tinox-eclipse/`, räumlich das Gegenstück zu `tinox/eclipse/`.

```
tinox/
├── eclipse/                ← bestehende IDE-Integration (nicht anfassen)
├── vscode/                 ← NEU: VS-Code-Integration (dieses Projekt)
│   ├── PLAN_CLI.md
│   ├── package.json        ← später
│   └── src/                ← später
├── crates/
├── Cargo.toml
└── …
```

Regeln daraus:

- Scaffolding und alle Extension-Dateien **nur** unter `tinox/vscode/`.
- `tinox/eclipse/`, `tinox/crates/`, `Cargo.toml`, Runtime und Tests bleiben unangetastet.
- Die Extension ist ein Node/TypeScript-Projekt **im** Tinox-Repo, kein zweites Repo auf der Workspace-Wurzel `jGrep4VSCode/`.

---

## 1. VS Code Extension Setup (TypeScript)

### 1.1 Generator und Sprache

Späteres Scaffolding (jetzt **nicht** ausführen) **im Ordner `tinox/vscode/`**, nicht auf der Workspace-Wurzel und nicht in `tinox/` selbst:

```bash
# im Tinox-Repo:
cd vscode
# Ordner existiert bereits (dieses PLAN_CLI.md). Generator so füttern,
# dass er in das aktuelle Verzeichnis schreibt, statt vscode/vscode/ anzulegen.
npx --package yo --package generator-code -- yo code
```

Empfohlene Generator-Antworten:

- **New Extension (TypeScript)**
- Name: `jgrep` (Anzeigename z. B. „jgrep“)
- Identifier: `jgrep` (Publisher später, z. B. `subnix`)
- Description: VS Code-Wrapper für das lokal installierte `jgrep`-CLI
- Initialize git: **nein** — `tinox/` ist bereits ein Git-Repo
- Bundler: zunächst **unbundled** (`tsc`). Webpack/esbuild erst, wenn das Bundle nötig wird.
- Linter: ESLint ja
- Tests: ja (Mocha/VS Code Test Runner), auch wenn die erste Version nur Gerüst-Tests hat
- Package manager: npm

Falls `yo code` hartnäckig einen Unterordner `jgrep/` anlegt: Inhalt nach `tinox/vscode/` heben, sodass `package.json` direkt in `tinox/vscode/package.json` liegt. Kein `tinox/vscode/jgrep/` (keine PDE-artige Extra-Ebene ohne Grund).

Begründung TypeScript: `@types/vscode` und `@types/node` decken `child_process`, `vscode.window` und `workspace.getConfiguration` typsicher ab.

### 1.2 Zielstruktur

```
tinox/
├── eclipse/                         ← bestehend, unverändert
│   ├── README.md
│   ├── SETUP.md
│   └── tinox-eclipse/
└── vscode/                          ← Extension-Wurzel (wie eclipse/tinox-eclipse/)
    ├── PLAN_CLI.md                  ← dieses Dokument
    ├── README.md                    ← später: Extension-Nutzung
    ├── package.json
    ├── tsconfig.json
    ├── .vscodeignore
    ├── package-lock.json
    ├── .vscode/
    │   ├── launch.json              ← Extension Development Host (F5)
    │   └── tasks.json               ← npm: watch / compile
    └── src/
        ├── extension.ts             ← activate / deactivate, Command-Registrierung
        ├── cli.ts                   ← spawn-Wrapper, Timeout, ENOENT
        ├── commands.ts              ← Editor lesen, Filter abfragen, Ergebnis zeigen
        └── config.ts                ← jgrep.path und spätere Settings lesen
```

Verantwortlichkeiten klein halten:

- `extension.ts` kennt nur Lifecycle und verdrahtet Commands.
- `config.ts` liest ausschließlich VS-Code-Settings.
- `cli.ts` kennt kein VS-Code-UI, nur Node-`child_process`.
- `commands.ts` verbindet Editor, InputBox, Output und `cli.ts`.

Das hält den Spawn-Pfad unit-testbar (Binary und Args injizieren).

### 1.3 Gitignore im Tinox-Repo

`tinox/.gitignore` ignoriert bereits **jedes** Verzeichnis namens `.vscode/` (Editor-Ordner). Die Extension braucht aber `vscode/.vscode/launch.json` und `tasks.json` im Repo — analog zu den eingecheckten Eclipse-Projektdateien.

Bei der späteren Umsetzung `.gitignore` ergänzen (nicht jetzt):

```gitignore
# VS Code extension (tinox/vscode/) — Build-Artefakte
vscode/node_modules/
vscode/out/
vscode/*.vsix

# launch.json / tasks.json der Extension versionieren
!vscode/.vscode/
!vscode/.vscode/launch.json
!vscode/.vscode/tasks.json
```

Zusätzlich in `tinox/vscode/.gitignore` (Generator-Default): `node_modules/`, `out/`, `*.vsix`, `.vscode-test/`.

`tinox/.gitignore` enthält außerdem sehr breite Muster (`test`, `test_*`). Testdateien der Extension nicht `test.ts` auf der Wurzel von `vscode/` nennen, sondern unter `src/test/` lassen — dort greifen die Muster in der Regel nicht als Dateiname `test`.

### 1.4 Manifest (`package.json`) — Extension-Metadaten

Wesentliche Felder, die das Scaffolding setzen bzw. die wir anpassen. Datei: **`tinox/vscode/package.json`**.

| Feld | Wert / Bedeutung |
| --- | --- |
| `name` | `jgrep` |
| `displayName` | `jgrep` |
| `publisher` | festlegen vor Marketplace-Publish |
| `engines.vscode` | z. B. `^1.85.0` (aktuelle Stable; Generator-Default übernehmen) |
| `categories` | `["Other"]` oder `["Programming Languages"]` |
| `main` | `./out/extension.js` (tsc-Ausgabe unter `tinox/vscode/out/`) |
| `activationEvents` | leer lassen, wenn Commands in `contributes.commands` stehen — VS Code aktiviert on-command automatisch |
| `contributes.commands` | mindestens `jgrep.filterDocument` |
| `contributes.configuration` | siehe Abschnitt 3 |
| `scripts` | `compile` (`tsc -p ./`), `watch`, `lint`, `vscode:prepublish` |
| `devDependencies` | `@types/vscode`, `@types/node`, `typescript`, `eslint`, `@vscode/test-electron` |

Kein `jgrep`-Binary in `dependencies`. Die Extension setzt eine lokale Installation voraus.

`package.json` der Extension darf nicht mit einem imaginären Root-Manifest auf `jGrep4VSCode/` verwechselt werden. npm-Skripte immer mit Arbeitsverzeichnis `tinox/vscode/` ausführen.

### 1.5 TypeScript-Compiler

`tinox/vscode/tsconfig.json` (Generator-Default ist ausreichend):

- `module`: `Node16` oder `commonjs` (zum Generator passend)
- `target`: `ES2022`
- `outDir`: `out` (also `tinox/vscode/out/`)
- `rootDir`: `src`
- `strict`: `true`
- `lib`: `ES2022`
- `sourceMap`: `true` (Debug im Extension Host)
- `skipLibCheck`: `true`

Nur `tinox/vscode/src/` ist Compilationswurzel. Der Tinox-Rust-Workspace (`crates/`, `Cargo.toml`) bleibt ein separates Build-System. Kein gemeinsames `tsconfig` auf `tinox/`-Ebene.

### 1.6 Aktivierung und Commands

Mindestens ein Command:

| Command-ID | Titel | Zweck |
| --- | --- | --- |
| `jgrep.filterDocument` | jgrep: Filter auf aktiven Editor anwenden | Puffer → Stdin → jgrep → Ergebnis |

Optional in einer späteren Ausbaustufe (nicht Teil der ersten Umsetzung, nur merken):

- `jgrep.filterSelection` — nur Selektion statt ganzem Dokument
- `jgrep.checkBinary` — `jgrep --version` zur Diagnose

Command Palette über `contributes.commands`. Optionales Keybinding (z. B. `ctrl+alt+j`) erst, wenn der Basisfluss steht — nicht mit Standard-Shortcuts kollidieren.

`activate(context)`:

1. Output-Channel `jgrep` einmal anlegen und in `context.subscriptions` legen.
2. Commands mit `vscode.commands.registerCommand` registrieren und ebenfalls subscriben.
3. Kein Spawn in `activate` — erst bei Command-Ausführung.

`deactivate()`: laufenden Child-Prozess killen, falls einer hängt (Referenz in einem kleinen Laufzeit-State).

### 1.7 Debug- und Build-Workflow (später)

Zum Entwickeln den Ordner **`tinox/vscode/`** als VS-Code-Workspace öffnen (oder ein Multi-Root-Workspace mit `tinox` + `tinox/vscode`). `F5` muss die `launch.json` **dieser** Extension treffen, nicht irgendwelche Editor-Settings unter einem ignorierten `.vscode/` im Tinox-Root.

- `F5` startet den Extension Development Host (`tinox/vscode/.vscode/launch.json`, `preLaunchTask`: `npm: watch` oder `compile`).
- Änderungen an `src/` → tsc watch → Host per Reload (`Ctrl+R`) neu laden.
- Manueller Smoke-Test: JSON-Datei **nicht speichern**, Filter `.` bzw. `.name`, Ergebnis muss den Pufferstand zeigen.

`.vscodeignore` (relativ zur Extension-Wurzel `tinox/vscode/`):

- `src/`
- `tsconfig.json`
- `.vscode/`
- `src/test/`, `.vscode-test/`
- `PLAN_CLI.md`
- `node_modules/` (üblicherweise implizit)

Parent-Pfade wie `../eclipse` oder `../crates` gehören **nicht** ins VSIX, weil das Paket von `tinox/vscode/` aus geschnürt wird — nicht vom Tinox-Root. Kein versehentliches Einpacken des Compilers.

### 1.8 Was das Setup bewusst nicht tut

- Kein Mitliefern oder Bauen der `jgrep`-Binary.
- Kein Aufruf von `tinox build` aus der Extension.
- Kein eingebettetes jq und kein WASM-Port.
- Keine Änderung an `tinox/eclipse/`, `tinox/crates/`, Cargo-Workspace oder Tinox-Runtime.
- Keine Extension-Dateien auf der Workspace-Wurzel `jGrep4VSCode/` und keine auf `tinox/` direkt neben `Cargo.toml`.

---

## 2. Child-Process: Editorinhalt über Stdin an jgrep

### 2.1 `exec` vs. `spawn` — Entscheidung: `spawn`

| | `child_process.exec` | `child_process.spawn` |
| --- | --- | --- |
| Shell | Ja (string-command) | Nein, Binary + Arg-Array |
| Stdin | Umständlich, nicht der vorgesehene Weg | `child.stdin` als Writable |
| Ausgabe | Komplett im Speicher, Default-`maxBuffer` 1 MiB | Streaming von stdout/stderr |
| Filter wie `.foo \| .bar` | Shell metacharacters, Injection | Sicheres Argv |
| Große ungespeicherte Dateien | Puffer-Limit, Abbruch | Geeignet |

**Wir nutzen `spawn`, nicht `exec`.**

`execFile` wäre ein Mittelweg (kein Shell, Args als Array), puffert aber ebenfalls und ist für Stdin unbequemer. Für den Editor-Wrapper ist `spawn` die klare Wahl.

Niemals `shell: true`. Der Filter kommt aus einer InputBox und darf nicht durch eine Shell.

### 2.2 Editortext: In-Memory, nicht Disk

Entscheidender VS-Code-Punkt:

```text
const editor = vscode.window.activeTextEditor;
if (!editor) { /* Fehler, abbrechen */ }
const input = editor.document.getText();
```

`TextDocument.getText()` liefert den **aktuellen Puffer**, inklusive ungespeicherter Änderungen (`isDirty === true`). Das gilt auch für Untitled-Dokumente ohne Dateipfad.

Was wir **nicht** tun:

- `fs.readFile(editor.document.uri.fsPath)` — das ist der Stand auf Disk.
- jgrep einen `FILE`-Pfad übergeben — jgrep würde die Datei öffnen, nicht den Puffer.
- Puffer in eine Temp-Datei schreiben und den Pfad übergeben — unnötig, Race mit Löschen, und Untitled-Docs haben keinen sinnvollen Pfad.

Selektion: erste Version nimmt immer das **ganze Dokument**. `getText(editor.selection)` nur als spätere Option, und nur wenn die Selektion nicht leer ist.

### 2.3 Argumente an jgrep

Aufrufform, die Stdin erzwingt (kein Dateiargument):

```text
<jgrep.path> [--no-color] [--pretty?] [--json|--yaml] <FILTER>
```

Regeln:

1. Binary aus Setting `jgrep.path` (Default `jgrep`). Relativer Name wird über `PATH` der Extension-Host-Umgebung aufgelöst.
2. Immer `--no-color` (oder `--color never`), damit Output Channel / Ergebnisdokument keine Escape-Sequenzen enthalten.
3. Optional `--pretty`, wenn ein späteres Setting `jgrep.pretty` (Default `true` für die UI) gesetzt ist. Im ersten Wurf kann `--pretty` fest an sein.
4. Format-Flag aus `editor.document.languageId`:
   - `json`, `jsonc`, `jsonl` → `--json`
   - `yaml`, `yml` → `--yaml`
   - sonst: `--json` als sicherer Default (jgrep ist das JSON-Tool). Nutzer mit YAML ohne Language-ID können später ein Setting `jgrep.inputFormat` (`auto` \| `json` \| `yaml`) bekommen.
5. Letztes Argument: der Filter-String aus der InputBox. Auch `.` explizit übergeben.
6. **Kein** Dateipfad, **kein** `-f` (Filterdatei), **kein** `-r` / `-l` (Datei-Semantik).

`-u` nicht setzen: der Puffer ist endlich; wir schließen Stdin nach dem Schreiben. jgrep wartet dann auf EOF und verarbeitet das Dokument als Ganzes — richtig für pretty-printed, mehrzeiliges JSON. `-u` würde mehrzeiliges JSON zeilenweise zerlegen und zerbrechen.

### 2.4 Spawn-Ablauf

Sequenz in `src/cli.ts` (konzeptionell):

1. `spawn(binary, args, { stdio: ['pipe', 'pipe', 'pipe'], env: process.env, windowsHide: true })`
2. Listener auf `error` **sofort** — `ENOENT` heißt: Binary nicht gefunden / nicht ausführbar.
3. stdout und stderr als UTF-8-Strings sammeln (`setEncoding('utf8')` oder Buffer concat).
4. `child.stdin.write(input, 'utf8')`, danach `child.stdin.end()`.
   - `write` kann `false` liefern (Backpressure): auf `'drain'` warten, dann `end()`.
   - Fehler auf `stdin` (EPIPE, falls jgrep schon tot ist) nicht als unhandled Rejection stehen lassen.
5. Auf `'close'` warten (nicht nur `'exit'`), damit die Streams geleert sind.
6. Rückgabe: `{ stdout, stderr, exitCode }`.

Empfohlene Spawn-Optionen:

- `stdio: ['pipe','pipe','pipe']` — wir brauchen alle drei.
- kein `cwd`-Zwang auf `tinox/` oder Workspace-Root; Default reicht, weil keine Dateiargumente.
- `windowsHide: true` — kein flackerndes Konsolenfenster unter Windows.
- Timeout (z. B. 30 s, später Setting `jgrep.timeoutMs`): bei Überschreitung `child.kill('SIGTERM')`, nach kurzer Gnadenfrist `SIGKILL`.
- Cancellation: Command bekommt `CancellationToken` bzw. wir verdrahten „Cancel“; Token → gleicher Kill-Pfad.

Node-`spawn` erbt `process.env` des Extension Host. Damit funktioniert Default `jgrep`, wenn die Binary auf dem **PATH des GUI-Prozesses** liegt. Unter macOS/Linux fehlt GUI-Apps oft der Shell-PATH — genau deshalb existiert `jgrep.path` als voller Pfad.

Die Eclipse-Seite macht dasselbe Muster für `tinox-lsp`: Preference „Pfad zur Binary“, Default ein Name auf PATH, Override als Absolutpfad. `jgrep.path` ist die VS-Code-Entsprechung zu `TinoxPreferencePage` / `TinoxPreferenceInitializer`.

### 2.5 Woher kommt der Filter?

Vor dem Spawn:

```text
vscode.window.showInputBox({
  prompt: 'jgrep-Filter (jq-Ausdruck)',
  value: lastFilter ?? '.',
  placeHolder: '.name  |  select(.level == "error")',
  ignoreFocusOut: true
})
```

- Abbrechen (undefined) → kein Spawn.
- Leerer String → als `.` behandeln.
- Letzten Filter in `context.workspaceState` merken (`jgrep.lastFilter`).

Nicht den Filter in die Shell interpolieren, nicht in Anführungszeichen wrappen. Er ist ein einziges argv-Element.

### 2.6 Ergebnis und Fehler zeigen

| Situation | UI |
| --- | --- |
| Exit 0, stdout nicht leer | Output-Channel `jgrep` zeigen und stdout dort ausgeben; optional zusätzlich „In neuem Editor öffnen“. Der Channel überschreibt keine offenen Dateien. |
| Exit 0, stdout leer | Information: kein Output (Filter hat `empty` geliefert oder Input war leer). |
| Exit ≠ 0 | `showErrorMessage` mit gekürztem stderr; volles stderr im Output-Channel. jgrep nutzt 2 für Nutzungs-/Parse-Fehler. |
| `ENOENT` | Klare Meldung: Binary unter `jgrep.path` nicht gefunden. Aktion „Einstellungen öffnen“ → `workbench.action.openSettings` mit Query `@id:jgrep.path`. |
| Kein aktiver Editor | Warnung, kein Spawn. |
| Timeout / Cancel | Warnung, Prozess beendet. |

`showErrorMessage` nicht mit dem gesamten stderr fluten. Channel hält das Detail.

### 2.7 Encoding und Größe

- Editor → Stdin: UTF-8. VS Code-Dokumente sind Unicode; `getText()` ist ein JS-String, `write(..., 'utf8')` ist korrekt.
- stdout/stderr: UTF-8. jgrep gibt JSON-Text.
- Sehr große Puffer: `getText()` materialisiert den ganzen String — akzeptabel für normale JSON/YAML-Dateien. Kein Extra-Streaming aus dem Editor nötig. stdout ebenfalls vollständig sammeln; bei Multi-Megabyte-Ergebnissen ist ein Untitled-Dokument besser als der Output-Channel.

### 2.8 Warum nicht die Datei speichern und den Pfad übergeben

1. Ungespeicherte Änderungen und Untitled-Buffer würden fehlen oder erst ein Save erzwingen.
2. Auto-Save oder Format-on-save wäre ein Seiteneffekt.
3. Stdin ist der von jgrep vorgesehene Weg für genau diesen Fall.

---

## 3. Configuration Setting: Pfad zur jgrep-Binary

### 3.1 Deklaration in `tinox/vscode/package.json`

Unter `contributes.configuration` (Skizze, keine Implementierung):

```json
"contributes": {
  "configuration": {
    "title": "jgrep",
    "properties": {
      "jgrep.path": {
        "type": "string",
        "default": "jgrep",
        "scope": "machine-overridable",
        "order": 0,
        "markdownDescription": "Pfad oder Programmname der `jgrep`-Binary. Standard `jgrep` sucht im `PATH` des VS-Code-Prozesses. Für ein lokales Build den absoluten Pfad setzen, z. B. `/usr/local/bin/jgrep` oder `C:\\\\Tools\\\\jgrep.exe`."
      }
    }
  }
}
```

Das entspricht dem Eclipse-Preference „Pfad zur Binary“ (`TinoxPreferencePage`), nur für `jgrep` statt `tinox-lsp`.

| Feld | Wahl | Grund |
| --- | --- | --- |
| Schlüssel | `jgrep.path` | VS Code splittet am ersten Punkt: Section `jgrep`, Property `path`. In Settings UI: **jgrep › Path**. |
| `type` | `string` | Name oder absoluter/relativer Pfad. |
| `default` | `"jgrep"` | Entspricht der gebauten Binary aus `tinox build … -o jgrep` und einem Eintrag im PATH. |
| `scope` | `machine-overridable` | Pfad hängt an der Maschine, darf aber im Workspace überschrieben werden (Projekt-Build). Alternative `machine`, wenn Workspace-Settings den Pfad nicht setzen sollen. |
| kein `format: uri` | — | Wäre für `file://` gedacht, nicht für `jgrep` oder `/usr/bin/jgrep`. |

User-/Workspace-Settings überschreiben den Default wie üblich:

```json
{
  "jgrep.path": "/home/user/src/jgrep-tinox/jgrep"
}
```

### 3.2 Lesen zur Laufzeit

In `src/config.ts`:

```text
function getJgrepPath(): string {
  const raw = vscode.workspace
    .getConfiguration('jgrep')
    .get<string>('path', 'jgrep');
  const trimmed = (raw ?? '').trim();
  return trimmed.length > 0 ? trimmed : 'jgrep';
}
```

Leerer String nach Trim → Fallback `jgrep`, nicht Spawn mit `""`.

`getConfiguration('jgrep')` respektiert die übliche Kaskade: Default → User → Workspace. Language-Override brauchen wir nicht.

### 3.3 Auflösung der Binary

- Ist `jgrep.path` nur ein Name (`jgrep`), sucht `spawn` in `process.env.PATH`.
- Ist es ein absoluter Pfad, wird genau diese Datei ausgeführt.
- Relativer Pfad ist gegen die cwd des Extension Host aufgelöst — unzuverlässig, und cwd ist **nicht** automatisch `tinox/vscode/`. Im README raten: **absoluten Pfad oder nackter Name**. Optional später: relative Pfade gegen `workspaceFolders[0].uri.fsPath` auflösen.

Windows:

- Nutzer trägt oft `jgrep.exe` oder `C:\…\jgrep.exe` ein.
- `spawn` startet `.exe` direkt. Kein `cmd.exe /c`.
- PATH-Suche hängt am PATH des **Code.exe**-Prozesses, nicht an Git-Bash.

Diagnose-Command (optional, zweite Iteration): `spawn(path, ['--version'])` ohne Stdin, stdout in einer Info-Message zeigen.

### 3.4 Weitere Settings — bewusst später

Nicht im ersten Schnitt nötig, aber konsistent im selben Namespace vorsehen:

| Schlüssel | Default | Zweck |
| --- | --- | --- |
| `jgrep.pretty` | `true` | `--pretty` setzen |
| `jgrep.timeoutMs` | `30000` | Spawn-Timeout |
| `jgrep.inputFormat` | `auto` | `auto` \| `json` \| `yaml` |
| `jgrep.extraArgs` | `[]` | Freie Extra-Flags — nur als `string[]`, nie ein String, der gesplittet wird |

Der geforderte erste Setting-Block ist ausschließlich `jgrep.path`.

### 3.5 Fehlermeldung, wenn die Binary fehlt

`error.code === 'ENOENT'` (und analog nicht ausführbar):

1. Output-Channel: versuchter Pfad, optional `PATH`, stderr falls vorhanden.
2. `showErrorMessage('jgrep-Binary nicht gefunden: …', 'Einstellungen öffnen')`.
3. Button öffnet die Settings-Suche nach `jgrep.path`.

Keine stillen Fallbacks auf andere Namen (`jgrep-tinox`, `ygrep`), außer wir dokumentieren das später explizit.

---

## 4. End-to-End-Datenfluss

```
Nutzer: Command Palette → „jgrep: Filter auf aktiven Editor anwenden“
        │
        ▼
commands.ts: activeTextEditor vorhanden?
        │ nein → Warnung, Ende
        ▼
document.getText()                    ← ungespeicherter Puffer
languageId → --json / --yaml
        │
        ▼
InputBox Filter (Default: letzter Filter oder ".")
        │ Abbruch → Ende
        ▼
config.ts: jgrep.path
        │
        ▼
cli.ts: spawn(path, ['--no-color', '--pretty', '--json'| '--yaml', filter])
        stdin.write(puffer) ; stdin.end()
        │
        ├─ error ENOENT → Settings-Hinweis
        ├─ timeout / cancel → kill
        ├─ close exit≠0 → stderr im Channel + ErrorMessage
        └─ close exit=0 → stdout im Channel (optional neues Dokument)
```

---

## 5. Sicherheit und Plattform

- **Kein Shell.** Filter und Pfad sind argv, nicht Interpolationen in einem Command-String.
- **`jgrep.extraArgs`** falls später: `string[]` aus Settings, jedes Element ein Arg — kein `split(' ')`.
- Workspace-Settings können `jgrep.path` auf ein beliebiges Executable setzen. Das ist dasselbe Vertrauensmodell wie andere CLI-Wrapper (`eslint.path`, `python.path`) und wie der Binary-Pfad im Eclipse-Plugin.
- Environment: `process.env` durchreichen, damit jgrep `env.HOME` usw. wie in der Shell sieht.
- Linux/macOS/Windows: gleicher Spawn-Pfad; Windows nur `windowsHide` und `.exe`-Hinweis in der Setting-Beschreibung.

---

## 6. Nicht-Ziele dieses Plans

- Extension-Quellcode, `yo code`, `npm install` (kommen nach diesem Dokument, in einem eigenen Schritt, **in `tinox/vscode/`**).
- Bundling der jgrep-Binary oder des Tinox-Compilers.
- Vollständige jq-UI (Filter-History-Webview, Tree-View der JSON-Treffer, CodeLens).
- Language Server oder Syntax-Highlight für jq-Filter (das Eclipse-Plugin ist ein Tinox-LSP-Client — diese Extension ist ein jgrep-Wrapper, kein zweites `tinox-lsp`).
- ygrep als zweites Command — `--yaml` über `languageId` reicht für v1.
- Dateibaum rekursiv durchsuchen (`-r`): das wäre Disk, nicht Editorpuffer.
- Änderungen an `tinox/eclipse/` oder am Cargo-Workspace.

---

## 7. Empfohlene Implementierungsreihenfolge (nach diesem Dokument)

1. In `tinox/vscode/` mit `yo code` (TypeScript) scaffolden; `package.json` muss direkt in `tinox/vscode/` liegen. `eclipse/` und `crates/` nicht anfassen.
2. `tinox/.gitignore` um `vscode/node_modules/`, `vscode/out/` und die `!vscode/.vscode/`-Ausnahmen ergänzen.
3. `contributes.configuration` für `jgrep.path` in `tinox/vscode/package.json` eintragen.
4. `src/cli.ts` mit `spawn` + Stdin/`ENOENT`/Timeout.
5. Command `jgrep.filterDocument`: `getText()` → InputBox → spawn → Output-Channel.
6. Manuell prüfen: Workspace-Ordner `tinox/vscode/` öffnen, `F5`, ungespeicherte JSON-Änderung erscheint im Ergebnis; fehlende Binary öffnet den Setting-Hinweis.
7. `tinox/vscode/README.md`: Voraussetzung „jgrep auf PATH oder `jgrep.path`“, Hinweise analog zu `eclipse/README.md` / `SETUP.md`.

Schritt 1–7 sind **nicht** Teil der aktuellen Aufgabe. Aktuell existiert nur dieser Plan unter `tinox/vscode/PLAN_CLI.md`.
