import type { LanguageTarget, TargetId } from "./languageTargets";

export interface TargetSettings {
  readonly enabled: boolean;
  readonly upstreamCommand: string;
  readonly upstreamArgs: string[];
}

export interface ExtensionSettings {
  readonly nexPath: string;
  readonly baseRef: string;
  readonly eventPollMs: number;
  readonly targets: Record<TargetId, TargetSettings>;
}

export interface ConfigurationReader {
  get<T>(section: string, defaultValue: T): T;
}

export function readSettings(config: ConfigurationReader): ExtensionSettings {
  return {
    nexPath: config.get("nexPath", "nex-lsp").trim() || "nex-lsp",
    baseRef: config.get("baseRef", "HEAD~1").trim() || "HEAD~1",
    eventPollMs: Math.max(100, config.get("eventPollMs", 500)),
    targets: {
      typescript: readTargetSettings(config, "typescript", "typescript-language-server", [
        "--stdio",
      ]),
      python: readTargetSettings(config, "python", "pyright-langserver", ["--stdio"]),
      rust: readTargetSettings(config, "rust", "rust-analyzer", []),
    },
  };
}

export function buildServerArgs(
  settings: ExtensionSettings,
  repoPath: string,
  target: LanguageTarget,
  targetSettings: TargetSettings,
): string[] {
  const args = [
    "--repo-path",
    repoPath,
    "--base-ref",
    settings.baseRef,
    "--event-poll-ms",
    String(settings.eventPollMs),
  ];

  const upstreamCommand = targetSettings.upstreamCommand.trim();
  if (upstreamCommand.length > 0) {
    args.push("--upstream-command", upstreamCommand);
    for (const arg of normalizeStringArray(targetSettings.upstreamArgs)) {
      args.push("--upstream-arg", arg);
    }
  }

  return args;
}

function readTargetSettings(
  config: ConfigurationReader,
  key: TargetId,
  defaultCommand: string,
  defaultArgs: string[],
): TargetSettings {
  return {
    enabled: config.get(`${key}.enabled`, true),
    upstreamCommand: config.get(`${key}.upstreamCommand`, defaultCommand).trim(),
    upstreamArgs: normalizeStringArray(config.get(`${key}.upstreamArgs`, defaultArgs)),
  };
}

function normalizeStringArray(value: string[]): string[] {
  return value
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}
