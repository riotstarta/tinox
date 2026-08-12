# jgrep for Visual Studio Code

Filter the **current editor** with the locally installed [jgrep](https://github.com/subnix-work/jgrep-tinox) CLI. The extension sends the in-memory buffer over stdin, so unsaved JSON and YAML are included.

This extension does **not** bundle jgrep. Install the `jgrep` binary separately and put it on your `PATH`, or set `jgrep-tinox.executablePath`.

## Features

- Command Palette: **jgrep: Filter current JSON/YAML**
- Editor context menu (right-click) on JSON and YAML files
- jq-style filter prompt
- Result opens as an unsaved JSON document beside the editor

## Requirements

- [jgrep](https://github.com/subnix-work/jgrep-tinox) installed locally (`jgrep --version`)

## Usage

1. Open a JSON or YAML document (it does not need to be saved).
2. Run **jgrep: Filter current JSON/YAML** from the Command Palette, or right-click in the editor.
3. Enter a filter such as `.`, `.name`, or `select(.level == "error")`.

## Extension Settings

| Setting | Default | Description |
| --- | --- | --- |
| `jgrep-tinox.executablePath` | `jgrep` | Path or name of the `jgrep` binary |

## Develop

Open this folder (`tinox/vscode/`) and press `F5` to launch an Extension Development Host.
