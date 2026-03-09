import * as fs from "node:fs";
import * as path from "node:path";
import { spawn } from "node:child_process";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  RevealOutputChannelOn,
  ServerOptions,
} from "vscode-languageclient/node";
import { buildServerArgs, readSettings, type TargetSettings } from "./config";
import { LANGUAGE_TARGETS, type LanguageTarget, type TargetId, targetForLanguage } from "./languageTargets";

interface EventStreamParams {
  eventId: string;
  description: string;
  agentId: string;
  tags: string[];
  timestamp: string;
}

class ClientManager implements vscode.Disposable {
  private readonly clients = new Map<string, LanguageClient>();
  private readonly starting = new Map<string, Promise<LanguageClient | undefined>>();
  private readonly seenEventIds = new Set<string>();

  constructor(
    private readonly output: vscode.OutputChannel,
    private readonly extensionUri: vscode.Uri,
  ) {}

  dispose(): void {
    void this.stopAll();
  }

  async ensureForDocument(document: vscode.TextDocument): Promise<LanguageClient | undefined> {
    if (document.uri.scheme !== "file") {
      return undefined;
    }

    const target = targetForLanguage(document.languageId);
    if (!target) {
      return undefined;
    }

    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    if (!folder) {
      return undefined;
    }

    const key = clientKey(folder, target.id);
    const existing = this.clients.get(key);
    if (existing) {
      return existing;
    }

    const inFlight = this.starting.get(key);
    if (inFlight) {
      return inFlight;
    }

    const startPromise = this.startClient(folder, target)
      .catch(async (error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        this.output.appendLine(`[error] Failed to start ${target.label} client for ${folder.name}: ${message}`);
        const selection = await vscode.window.showWarningMessage(
          `Nexum Graph could not start for ${target.label} in ${folder.name}.`,
          "Show Output",
          "Open Extension README",
        );
        if (selection === "Show Output") {
          this.output.show(true);
        } else if (selection === "Open Extension README") {
          void vscode.commands.executeCommand(
            "vscode.open",
            vscode.Uri.joinPath(this.extensionUri, "README.md"),
          );
        }
        return undefined;
      })
      .finally(() => {
        this.starting.delete(key);
      });

    this.starting.set(key, startPromise);
    return startPromise;
  }

  async showSemanticDiffForActiveEditor(): Promise<void> {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
      void vscode.window.showInformationMessage("Nexum Graph needs an active editor to request semantic diff.");
      return;
    }

    const client = await this.ensureForDocument(editor.document);
    if (!client) {
      void vscode.window.showWarningMessage(
        `Nexum Graph is not active for ${editor.document.languageId} in this workspace.`,
      );
      return;
    }

    const diff = await client.sendRequest("nex/semanticDiff", {
      uri: editor.document.uri.toString(),
    });
    this.output.appendLine(`[semantic-diff] ${editor.document.uri.fsPath}`);
    this.output.appendLine(JSON.stringify(diff, null, 2));
    this.output.show(true);
  }

  async restartAll(): Promise<void> {
    await this.stopAll();
    const documents = vscode.workspace.textDocuments.filter((document) => document.uri.scheme === "file");
    for (const document of documents) {
      await this.ensureForDocument(document);
    }
    if (documents.length === 0) {
      this.output.appendLine("[info] Nexum Graph clients cleared. Open a supported file to start them again.");
    }
  }

  async stopAll(): Promise<void> {
    const clients = [...this.clients.values()];
    this.clients.clear();
    this.starting.clear();
    await Promise.all(
      clients.map(async (client) => {
        try {
          await client.stop();
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          this.output.appendLine(`[warn] Failed to stop client cleanly: ${message}`);
        }
      }),
    );
  }

  private async startClient(
    folder: vscode.WorkspaceFolder,
    target: LanguageTarget,
  ): Promise<LanguageClient | undefined> {
    const config = readSettings(vscode.workspace.getConfiguration("nexumGraph", folder.uri));
    const targetSettings = config.targets[target.id];
    if (!targetSettings.enabled) {
      return undefined;
    }

    if (!(await commandExists(config.nexPath, folder.uri.fsPath))) {
      throw new Error(
        `Could not find '${config.nexPath}'. Build nex-lsp and set nexumGraph.nexPath if needed.`,
      );
    }

    const effectiveTargetSettings = await this.resolveUpstreamSettings(folder, target, targetSettings);
    const args = buildServerArgs(config, folder.uri.fsPath, target, effectiveTargetSettings);
    const selector = target.languages.map((language) => ({
      language,
      pattern: new vscode.RelativePattern(folder, "**/*"),
      scheme: "file",
    }));

    const serverOptions: ServerOptions = {
      command: config.nexPath,
      args,
      options: {
        cwd: folder.uri.fsPath,
      },
    };

    const clientOptions: LanguageClientOptions = {
      documentSelector:
        selector as unknown as NonNullable<LanguageClientOptions["documentSelector"]>,
      outputChannel: this.output,
      revealOutputChannelOn: RevealOutputChannelOn.Never,
      workspaceFolder: folder,
      synchronize: {
        configurationSection: "nexumGraph",
      },
    };

    const client = new LanguageClient(
      `nexumGraph.${folder.name}.${target.id}`,
      `Nexum Graph (${folder.name} / ${target.label})`,
      serverOptions,
      clientOptions,
    );
    this.registerNotifications(client, folder, target);
    await client.start();
    this.clients.set(clientKey(folder, target.id), client);
    this.output.appendLine(
      `[start] ${folder.name} ${target.label} via ${config.nexPath}${formatUpstreamSuffix(effectiveTargetSettings)}`,
    );
    return client;
  }

  private registerNotifications(
    client: LanguageClient,
    folder: vscode.WorkspaceFolder,
    target: LanguageTarget,
  ): void {
    client.onNotification("nex/eventStream", (params: EventStreamParams) => {
      if (this.seenEventIds.has(params.eventId)) {
        return;
      }
      this.seenEventIds.add(params.eventId);
      this.output.appendLine(
        `[event] ${folder.name} ${target.label} ${params.timestamp} ${params.agentId}: ${params.description}`,
      );
    });
  }

  private async resolveUpstreamSettings(
    folder: vscode.WorkspaceFolder,
    target: LanguageTarget,
    targetSettings: TargetSettings,
  ): Promise<TargetSettings> {
    const command = targetSettings.upstreamCommand.trim();
    if (command.length === 0) {
      return targetSettings;
    }

    if (await commandExists(command, folder.uri.fsPath)) {
      return targetSettings;
    }

    this.output.appendLine(
      `[warn] ${target.label} upstream '${command}' was not found. Starting overlay-only mode for ${folder.name}.`,
    );
    void vscode.window.showWarningMessage(
      `Nexum Graph could not find '${command}' for ${target.label}. Starting overlay-only mode instead.`,
    );
    return {
      ...targetSettings,
      upstreamCommand: "",
      upstreamArgs: [],
    };
  }
}

let manager: ClientManager | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel("Nexum Graph");
  manager = new ClientManager(output, context.extensionUri);
  context.subscriptions.push(output, manager);

  context.subscriptions.push(
    vscode.commands.registerCommand("nexumGraph.showSemanticDiff", async () => {
      await manager?.showSemanticDiffForActiveEditor();
    }),
    vscode.commands.registerCommand("nexumGraph.restartLanguageClients", async () => {
      await manager?.restartAll();
    }),
    vscode.commands.registerCommand("nexumGraph.showOutput", () => {
      output.show(true);
    }),
    vscode.workspace.onDidOpenTextDocument((document) => {
      void manager?.ensureForDocument(document);
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor) {
        void manager?.ensureForDocument(editor.document);
      }
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("nexumGraph")) {
        output.appendLine("[info] Nexum Graph configuration changed. Restarting language clients.");
        void manager?.restartAll();
      }
    }),
  );

  for (const document of vscode.workspace.textDocuments) {
    await manager.ensureForDocument(document);
  }
  if (vscode.window.activeTextEditor) {
    await manager.ensureForDocument(vscode.window.activeTextEditor.document);
  }
}

export async function deactivate(): Promise<void> {
  if (manager) {
    await manager.stopAll();
    manager = undefined;
  }
}

function clientKey(folder: vscode.WorkspaceFolder, targetId: TargetId): string {
  return `${folder.uri.toString()}::${targetId}`;
}

function formatUpstreamSuffix(targetSettings: TargetSettings): string {
  if (!targetSettings.upstreamCommand) {
    return " (overlay-only)";
  }
  return ` (upstream: ${targetSettings.upstreamCommand})`;
}

async function commandExists(command: string, cwd: string): Promise<boolean> {
  if (command.length === 0) {
    return false;
  }

  if (isFilePath(command)) {
    return fs.existsSync(command);
  }

  const probeCommand = process.platform === "win32" ? "where" : "which";
  return await new Promise<boolean>((resolve) => {
    const child = spawn(probeCommand, [command], {
      cwd,
      windowsHide: true,
    });
    child.on("close", (code) => resolve(code === 0));
    child.on("error", () => resolve(false));
  });
}

function isFilePath(command: string): boolean {
  return path.isAbsolute(command) || command.includes("/") || command.includes("\\");
}
