import { spawn } from 'node:child_process';
import { closeSync, openSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import * as vscode from 'vscode';

const COMMAND_ID = 'jgrep-tinox.filter';
const CONFIG_SECTION = 'jgrep-tinox';
const CONFIG_EXECUTABLE_PATH = 'executablePath';
const DEFAULT_BINARY = 'jgrep';

interface JgrepRunResult {
	stdout: string;
	stderr: string;
	exitCode: number;
}

export function activate(context: vscode.ExtensionContext): void {
	const disposable = vscode.commands.registerCommand(COMMAND_ID, () => runFilterCommand());
	context.subscriptions.push(disposable);
}

export function deactivate(): void {
}

async function runFilterCommand(): Promise<void> {
	const editor = vscode.window.activeTextEditor;
	if (!editor) {
		void vscode.window.showErrorMessage('Kein Editor geöffnet.');
		return;
	}

	const input = editor.document.getText();
	if (input.length === 0) {
		void vscode.window.showErrorMessage('Der aktive Editor enthält keinen Text.');
		return;
	}

	const query = await vscode.window.showInputBox({
		prompt: 'jgrep-Filter (jq-Ausdruck)',
		placeHolder: '.name  |  select(.level == "error")',
		ignoreFocusOut: true,
	});
	if (query === undefined) {
		return;
	}
	const filter = query.trim() === '' ? '.' : query;

	const executablePath = getExecutablePath();

	let result: JgrepRunResult;
	try {
		result = await runJgrep(executablePath, filter, input, editor.document.languageId);
	} catch (err) {
		void vscode.window.showErrorMessage(formatSpawnError(err, executablePath));
		return;
	}

	if (result.exitCode !== 0) {
		const detail = result.stderr.trim() || result.stdout.trim();
		if (result.exitCode === 1 && detail.length === 0) {
			void vscode.window.showInformationMessage('Keine Treffer für diesen Filter.');
			return;
		}
		const message = detail || `jgrep beendete sich mit Code ${result.exitCode}.`;
		void vscode.window.showErrorMessage(message);
		return;
	}

	const document = await vscode.workspace.openTextDocument({
		content: result.stdout,
		language: 'json',
	});
	await vscode.window.showTextDocument(document, vscode.ViewColumn.Beside);
}

function getExecutablePath(): string {
	const raw = vscode.workspace
		.getConfiguration(CONFIG_SECTION)
		.get<string>(CONFIG_EXECUTABLE_PATH);
	const trimmed = (raw ?? '').trim();
	return trimmed.length > 0 ? trimmed : DEFAULT_BINARY;
}

function inputFormatFlag(languageId: string): string {
	switch (languageId) {
		case 'yaml':
		case 'yml':
			return '--yaml';
		default:
			return '--json';
	}
}

function runJgrep(
	executablePath: string,
	filter: string,
	input: string,
	languageId: string,
): Promise<JgrepRunResult> {
	return new Promise((resolve, reject) => {
		// jgrep reads stdin via fileReadAllText("/dev/stdin"). Node's spawn
		// pipes are O_NONBLOCK; that read returns empty and jgrep exits 1
		// with no output. A regular temp file as the stdin fd is blocking
		// and still does not use the editor's on-disk path.
		const tmpPath = join(tmpdir(), `jgrep-stdin-${process.pid}-${Date.now()}.txt`);
		let fd: number;
		try {
			writeFileSync(tmpPath, input, 'utf8');
			fd = openSync(tmpPath, 'r');
			unlinkSync(tmpPath);
		} catch (err) {
			try {
				unlinkSync(tmpPath);
			} catch {
				// ignore cleanup failure
			}
			reject(err);
			return;
		}

		const args = ['--no-color', '--pretty', inputFormatFlag(languageId), filter];
		let child;
		try {
			child = spawn(executablePath, args, {
				stdio: [fd, 'pipe', 'pipe'],
				env: process.env,
				windowsHide: true,
			});
		} catch (err) {
			closeSync(fd);
			reject(err);
			return;
		}
		closeSync(fd);

		let stdout = '';
		let stderr = '';
		let settled = false;

		const settle = (fn: () => void): void => {
			if (settled) {
				return;
			}
			settled = true;
			fn();
		};

		child.on('error', (err: Error) => {
			settle(() => reject(err));
		});

		if (child.stdout) {
			child.stdout.setEncoding('utf8');
			child.stdout.on('data', (chunk: string) => {
				stdout += chunk;
			});
		}
		if (child.stderr) {
			child.stderr.setEncoding('utf8');
			child.stderr.on('data', (chunk: string) => {
				stderr += chunk;
			});
		}

		child.on('close', (code) => {
			settle(() => resolve({
				stdout,
				stderr,
				exitCode: code ?? 1,
			}));
		});
	});
}

function formatSpawnError(err: unknown, executablePath: string): string {
	const code = err && typeof err === 'object' && 'code' in err ? String((err as NodeJS.ErrnoException).code) : '';
	if (code === 'ENOENT') {
		return `jgrep-Binary nicht gefunden: ${executablePath}`;
	}
	if (err instanceof Error && err.message) {
		return err.message;
	}
	return `jgrep konnte nicht gestartet werden: ${executablePath}`;
}
