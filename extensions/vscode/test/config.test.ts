import test from "node:test";
import assert from "node:assert/strict";
import { buildServerArgs, readSettings, type ConfigurationReader } from "../src/config";
import { LANGUAGE_TARGETS, targetForLanguage } from "../src/languageTargets";

class MockConfig implements ConfigurationReader {
  constructor(private readonly values: Record<string, unknown>) {}

  get<T>(section: string, defaultValue: T): T {
    return (this.values[section] as T | undefined) ?? defaultValue;
  }
}

test("targetForLanguage maps supported languages", () => {
  assert.equal(targetForLanguage("typescript")?.id, "typescript");
  assert.equal(targetForLanguage("python")?.id, "python");
  assert.equal(targetForLanguage("rust")?.id, "rust");
  assert.equal(targetForLanguage("go"), undefined);
  assert.equal(LANGUAGE_TARGETS.length, 3);
});

test("readSettings returns defaults", () => {
  const settings = readSettings(new MockConfig({}));

  assert.equal(settings.nexPath, "nex-lsp");
  assert.equal(settings.baseRef, "HEAD~1");
  assert.equal(settings.eventPollMs, 500);
  assert.equal(settings.targets.typescript.upstreamCommand, "typescript-language-server");
  assert.deepEqual(settings.targets.python.upstreamArgs, ["--stdio"]);
  assert.equal(settings.targets.rust.upstreamCommand, "rust-analyzer");
});

test("readSettings applies overrides and normalizes arrays", () => {
  const settings = readSettings(
    new MockConfig({
      nexPath: "C:\\tools\\nex-lsp.exe",
      baseRef: "origin/main",
      eventPollMs: 50,
      "python.enabled": false,
      "python.upstreamCommand": " pyright-langserver ",
      "python.upstreamArgs": [" --stdio ", "", " --lib "],
    }),
  );

  assert.equal(settings.nexPath, "C:\\tools\\nex-lsp.exe");
  assert.equal(settings.baseRef, "origin/main");
  assert.equal(settings.eventPollMs, 100);
  assert.equal(settings.targets.python.enabled, false);
  assert.equal(settings.targets.python.upstreamCommand, "pyright-langserver");
  assert.deepEqual(settings.targets.python.upstreamArgs, ["--stdio", "--lib"]);
});

test("buildServerArgs includes upstream configuration when present", () => {
  const settings = readSettings(new MockConfig({ baseRef: "main", eventPollMs: 900 }));
  const args = buildServerArgs(
    settings,
    "/repo",
    LANGUAGE_TARGETS[0],
    {
      enabled: true,
      upstreamCommand: "typescript-language-server",
      upstreamArgs: ["--stdio"],
    },
  );

  assert.deepEqual(args, [
    "--repo-path",
    "/repo",
    "--base-ref",
    "main",
    "--event-poll-ms",
    "900",
    "--upstream-command",
    "typescript-language-server",
    "--upstream-arg",
    "--stdio",
  ]);
});

test("buildServerArgs omits upstream flags in overlay-only mode", () => {
  const settings = readSettings(new MockConfig({}));
  const args = buildServerArgs(
    settings,
    "/repo",
    LANGUAGE_TARGETS[1],
    {
      enabled: true,
      upstreamCommand: "",
      upstreamArgs: [],
    },
  );

  assert.deepEqual(args, [
    "--repo-path",
    "/repo",
    "--base-ref",
    "HEAD~1",
    "--event-poll-ms",
    "500",
  ]);
});
