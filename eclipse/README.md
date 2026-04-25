# Tinox Eclipse Plugin

Eclipse-Plugin das den `tinox-lsp` Language Server einbindet.

## Features (via LSP)

- Fehler-Unterstreichungen (Diagnostics)
- Hover → Typen und Funktionssignaturen
- Ctrl+Space → Autocomplete
- F3 / Ctrl+Click → Go to Definition
- Outline-View → Document Symbols

## Setup

### 1. tinox-lsp installieren

```bash
./install-lsp.sh
# Installiert nach ~/.cargo/bin/tinox-lsp
```

### 2. Plugin in Eclipse laden

**Voraussetzung:** Eclipse IDE for Plugin Development (≥ 2023-09) mit PDE und LSP4E.

LSP4E installieren falls noch nicht vorhanden:
- Help → Install New Software
- Work with: `https://download.eclipse.org/lsp4e/releases/latest/`
- Install: "Language Server Protocol client for Eclipse"

Plugin importieren:
1. File → Import → Existing Projects into Workspace
2. Root directory: dieses Verzeichnis (`eclipse/tinox-eclipse`)
3. Finish

### 3. Plugin starten

1. Rechtsklick auf `tinox-eclipse` Projekt → Run As → Eclipse Application
2. Im neuen Eclipse-Fenster: Neues Projekt anlegen, Datei `*.tnx` erstellen
3. Language Server startet automatisch

### 4. Binary-Pfad konfigurieren

Window → Preferences → Tinox → Pfad zur `tinox-lsp` Binary setzen

## Projektstruktur

```
tinox-eclipse/
├── META-INF/MANIFEST.MF       # OSGi Bundle-Manifest
├── plugin.xml                 # Extension Points
├── build.properties
└── src/tinox/eclipse/
    ├── Activator.java                  # Plugin-Lifecycle
    ├── TinoxLanguageServer.java        # LSP Server Process
    ├── TinoxPreferencePage.java        # Settings UI
    └── TinoxPreferenceInitializer.java # Default-Werte
```
