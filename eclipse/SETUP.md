# Tinox Eclipse Plugin — Setup-Anleitung

Diese Anleitung erklärt Schritt für Schritt, wie du das Tinox Eclipse Plugin einrichtest und verwendest.

---

## Voraussetzungen

| Was | Version |
|-----|---------|
| Eclipse IDE for Plugin Development | ≥ 2023-09 |
| Java | ≥ 17 |
| Rust / Cargo | ≥ 1.75 |

---

## Schritt 1: tinox-lsp Binary installieren

Das Eclipse-Plugin kommuniziert mit dem `tinox-lsp` Language Server. Dieser muss zuerst gebaut und installiert werden.

```bash
# Im Wurzelverzeichnis des Repos:
./eclipse/install-lsp.sh
```

Das Skript baut `tinox-lsp` im Release-Modus und kopiert die Binary nach `~/.cargo/bin/tinox-lsp`.

**Manuell (alternativ):**
```bash
cargo build --release -p tinox-lsp
cp target/release/tinox-lsp ~/.cargo/bin/tinox-lsp
```

---

## Schritt 2: LSP4E in Eclipse installieren

LSP4E ist das Framework das Eclipse mit Language Servern verbindet.

1. Eclipse öffnen
2. **Help → Install New Software…**
3. Bei "Work with" eintragen:
   ```
   https://download.eclipse.org/lsp4e/releases/latest/
   ```
4. **"Language Server Protocol client for Eclipse"** auswählen
5. Next → Finish → Eclipse neu starten

---

## Schritt 3: Plugin in Eclipse importieren

1. **File → Import…**
2. **General → Existing Projects into Workspace** → Next
3. Root directory: Pfad zum `eclipse/tinox-eclipse` Ordner im Repo wählen
4. `tinox-eclipse` sollte in der Liste erscheinen → **Finish**

---

## Schritt 4: Plugin starten

1. Im Package Explorer: Rechtsklick auf `tinox-eclipse`
2. **Run As → Eclipse Application**
3. Ein zweites Eclipse-Fenster öffnet sich — das ist die Test-Instanz mit dem Plugin

---

## Schritt 5: Testen

Im zweiten Eclipse-Fenster:

1. **File → New → Project → General → Project** → Finish
2. Neue Datei anlegen: Rechtsklick auf Projekt → **New → File**, Name: `test.tnx`
3. Folgendes eingeben:

```tinox
fn add(a: Int64, b: Int64) -> Int64 {
    return a + b;
}

fn main() -> Int64 {
    let x = add(1, 2);
    return x;
}
```

**Was du jetzt siehst:**

| Aktion | Ergebnis |
|--------|----------|
| Tippfehler einbauen | Rote Unterstreichung erscheint |
| Cursor auf `add` in Zeile 6 | **Hover** zeigt `fn add(a: Int64, b: Int64) -> Int64` |
| Ctrl+Space | **Autocomplete** öffnet sich mit Keywords, Builtins, Funktionen |
| F3 auf `add` | **Go to Definition** springt zur Funktionsdeklaration |
| Window → Show View → Outline | **Outline-View** zeigt alle Funktionen und Klassen |

---

## Schritt 6 (Optional): Binary-Pfad konfigurieren

Falls `tinox-lsp` nicht unter `~/.cargo/bin/tinox-lsp` liegt:

1. **Window → Preferences → Tinox**
2. Pfad zur `tinox-lsp` Binary eintragen
3. OK → Eclipse neu starten

---

## Troubleshooting

**Language Server startet nicht**
- Prüfe ob die Binary ausführbar ist: `ls -la ~/.cargo/bin/tinox-lsp`
- Prüfe den Pfad in den Preferences (Schritt 6)
- Schau in **Window → Show View → Other → Language Servers** nach Fehlern

**Keine Fehler-Unterstreichungen**
- Dateiendung muss `.tnx` sein (nicht `.tnx`)
- Kurz warten — der Server braucht beim ersten Start 1-2 Sekunden

**`install-lsp.sh` schlägt fehl**
- Stelle sicher dass `cargo` im PATH ist: `which cargo`
- Baue manuell: `cargo build --release -p tinox-lsp`

---

## Plugin als `.jar` exportieren (für Weitergabe)

Um das Plugin ohne Eclipse-Entwicklungsumgebung zu verteilen:

1. Rechtsklick auf `tinox-eclipse` → **Export…**
2. **Plug-in Development → Deployable plug-ins and fragments**
3. Destination: Verzeichnis wählen → Finish
4. Die erzeugte `.jar` in den `dropins/`-Ordner einer Eclipse-Installation kopieren
