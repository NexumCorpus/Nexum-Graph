export type TargetId = "typescript" | "python" | "rust";

export interface LanguageTarget {
  readonly id: TargetId;
  readonly label: string;
  readonly languages: readonly string[];
  readonly defaultUpstreamCommand: string;
  readonly defaultUpstreamArgs: readonly string[];
}

export const LANGUAGE_TARGETS: readonly LanguageTarget[] = [
  {
    id: "typescript",
    label: "TypeScript",
    languages: ["javascript", "javascriptreact", "typescript", "typescriptreact"],
    defaultUpstreamCommand: "typescript-language-server",
    defaultUpstreamArgs: ["--stdio"],
  },
  {
    id: "python",
    label: "Python",
    languages: ["python"],
    defaultUpstreamCommand: "pyright-langserver",
    defaultUpstreamArgs: ["--stdio"],
  },
  {
    id: "rust",
    label: "Rust",
    languages: ["rust"],
    defaultUpstreamCommand: "rust-analyzer",
    defaultUpstreamArgs: [],
  },
];

const TARGET_BY_LANGUAGE = new Map<string, LanguageTarget>();
for (const target of LANGUAGE_TARGETS) {
  for (const language of target.languages) {
    TARGET_BY_LANGUAGE.set(language, target);
  }
}

export function targetForLanguage(languageId: string): LanguageTarget | undefined {
  return TARGET_BY_LANGUAGE.get(languageId);
}
