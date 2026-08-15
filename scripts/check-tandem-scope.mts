import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const BASE = "a2de8debc451828261e60fada9aa1a7a808e9961";
type ScopeChange = { path: string; content: string };

const evidenceOnly = (path: string) =>
  ["docs/", "tests/", ".superpowers/", "scratch/"].some((directory) =>
    path.startsWith(directory),
  );
const allowedPath = (path: string) =>
  [
    "package.json",
    "pnpm-lock.yaml",
    "playwright.config.ts",
    "vitest.config.ts",
    "scripts/check-tandem-scope.mts",
    "src/tandem/api/demoTaskGateway.ts",
    "src/tandem/demo/DemoLegacyConfigApp.tsx",
  ].includes(path) ||
  ["e2e/", "tests/tandem/", "tests/scripts/", ".superpowers/sdd/"].some(
    (directory) => path.startsWith(directory),
  );

export function inspectScopeChanges(changes: ScopeChange[]): string[] {
  const findings = [];
  for (const { path, content } of changes) {
    if (evidenceOnly(path)) continue;
    if (!allowedPath(path)) findings.push(path + ": out-of-scope path");
    if (
      path === "src-tauri/tauri.conf.json" &&
      /productName|identifier/.test(content)
    )
      findings.push(path + ": identity metadata");
    if (
      path !== "pnpm-lock.yaml" &&
      path !== "package.json" &&
      path !== "scripts/check-tandem-scope.mts"
    ) {
      if (/credential|api[_-]?key|secret|token/i.test(path + "\n" + content))
        findings.push(path + ": credentials");
      if (
        /session.{0,12}(scan|discover)|scan.{0,12}session/i.test(
          path + "\n" + content,
        )
      )
        findings.push(path + ": session scanning");
      if (/iterm2/i.test(path + "\n" + content))
        findings.push(path + ": iTerm2");
      if (
        /(preset|handoff).{0,20}comparison|comparison.{0,20}(preset|handoff)/i.test(
          path + "\n" + content,
        )
      )
        findings.push(path + ": Preset/Handoff/comparison");
    }
  }
  return [...new Set(findings)];
}

export function mergeChangedPaths(...outputs: string[]): string[] {
  return [
    ...new Set(outputs.flatMap((output) => output.split("\n").filter(Boolean))),
  ].sort();
}

function changedFiles(): string[] {
  const tracked = execFileSync("git", ["diff", "--name-only", BASE], {
    encoding: "utf8",
  });
  const untracked = execFileSync(
    "git",
    ["ls-files", "--others", "--exclude-standard"],
    { encoding: "utf8" },
  );
  return mergeChangedPaths(tracked, untracked);
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const changes = changedFiles().map((path) => ({
    path,
    content: readFileSync(path, "utf8"),
  }));
  const findings = inspectScopeChanges(changes);
  if (findings.length) {
    console.error(findings.join("\n"));
    process.exit(1);
  }
  console.log(
    "Tandem Task 7 scope guard passed (" +
      changes.length +
      " changed files inspected).",
  );
}
