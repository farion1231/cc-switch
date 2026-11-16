import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const ROOT = path.resolve(__dirname, "../..");

function registeredCommands(): string[] {
  const lib = readFileSync(path.join(ROOT, "src-tauri/src/lib.rs"), "utf8");
  const handler = lib.match(/generate_handler!\[([\s\S]*?)\]\)/);
  if (!handler) throw new Error("generate_handler! not found in lib.rs");
  return [...handler[1].matchAll(/^\s*([\w:]+),?\s*$/gm)].map(
    (m) => m[1].split("::").pop()!,
  );
}

function frontendSources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return frontendSources(full);
    return /\.(ts|tsx)$/.test(entry.name) ? [readFileSync(full, "utf8")] : [];
  });
}

describe("Tauri command registry", () => {
  it("registers only commands the frontend invokes", () => {
    const sources = frontendSources(path.join(ROOT, "src"));
    // invoke(`get_${app}_current_state`) and similar: turn each template into a pattern.
    const templates = sources.flatMap((source) =>
      [...source.matchAll(/invoke(?:<[^>]*>)?\(\s*`([^`]*)`/g)].map(
        (m) =>
          new RegExp(
            `^${m[1]
              .split(/\$\{[^}]*\}/)
              .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
              .join("\\w+")}$`,
          ),
      ),
    );
    const uncalled = registeredCommands().filter(
      (name) =>
        !sources.some((source) =>
          new RegExp(`["'\`/]${name}["'\`]`).test(source),
        ) && !templates.some((pattern) => pattern.test(name)),
    );
    expect(
      uncalled,
      "These commands are registered in lib.rs generate_handler! but nothing under src/ invokes them. Delete the command (and its registration), or call it from the frontend.",
    ).toEqual([]);
  });
});
