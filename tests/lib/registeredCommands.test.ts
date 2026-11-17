import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import ts from "typescript";
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

function frontendFiles(dir: string): ts.SourceFile[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return frontendFiles(full);
    if (!/\.(ts|tsx)$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name))
      return [];
    return [
      ts.createSourceFile(
        full,
        readFileSync(full, "utf8"),
        ts.ScriptTarget.Latest,
        true,
        entry.name.endsWith("x") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
      ),
    ];
  });
}

function walk(node: ts.Node, visit: (node: ts.Node) => void) {
  visit(node);
  node.forEachChild((child) => walk(child, visit));
}

/** The named function a string sits in: `api.method`, `function name`, `const name = () =>`. */
interface Wrapper {
  object?: string;
  name: ts.Identifier;
}

function enclosingWrapper(node: ts.Node): Wrapper | undefined {
  for (let n = node.parent; n; n = n.parent) {
    let name: ts.Node | undefined;
    let member: ts.Node | undefined;
    if (ts.isFunctionDeclaration(n)) name = n.name;
    else if (ts.isMethodDeclaration(n)) [name, member] = [n.name, n];
    else if (ts.isArrowFunction(n) || ts.isFunctionExpression(n)) {
      if (ts.isPropertyAssignment(n.parent))
        [name, member] = [n.parent.name, n.parent];
      else if (ts.isVariableDeclaration(n.parent)) name = n.parent.name;
    }
    if (!name || !ts.isIdentifier(name)) continue;
    const literal = member?.parent;
    const holder = literal?.parent;
    return holder &&
      ts.isVariableDeclaration(holder) &&
      ts.isIdentifier(holder.name)
      ? { object: holder.name.text, name }
      : { name };
  }
  return undefined;
}

function identifiersByName(files: ts.SourceFile[]) {
  const index = new Map<string, ts.Identifier[]>();
  for (const file of files)
    walk(file, (node) => {
      if (!ts.isIdentifier(node)) return;
      const named = index.get(node.text) ?? [];
      named.push(node);
      index.set(node.text, named);
    });
  return index;
}

/** `mcpApi` used as a value (passed around), so its members may be called as `api.x`. */
function escapes(object: string, index: Map<string, ts.Identifier[]>) {
  return (index.get(object) ?? []).some((node) => {
    const parent = node.parent;
    return !(
      (ts.isPropertyAccessExpression(parent) && parent.expression === node) ||
      (ts.isVariableDeclaration(parent) && parent.name === node) ||
      ts.isImportSpecifier(parent) ||
      ts.isExportSpecifier(parent) ||
      ts.isTypeQueryNode(parent)
    );
  });
}

/** Whether anything in src/ (outside tests) uses the wrapper. */
function isReferenced(
  wrapper: Wrapper,
  index: Map<string, ts.Identifier[]>,
): boolean {
  const { object, name } = wrapper;
  const anyAccess = object === undefined || escapes(object, index);
  return (index.get(name.text) ?? []).some((node) => {
    if (node === name) return false;
    if (object === undefined) return true;
    const parent = node.parent;
    if (ts.isPropertyAccessExpression(parent) && parent.name === node) {
      const owner = parent.expression;
      return (
        anyAccess ||
        owner.kind === ts.SyntaxKind.ThisKeyword ||
        (ts.isIdentifier(owner) && owner.text === object)
      );
    }
    if (ts.isBindingElement(parent)) {
      const holder = parent.parent.parent;
      return (
        ts.isVariableDeclaration(holder) &&
        !!holder.initializer &&
        ts.isIdentifier(holder.initializer) &&
        holder.initializer.text === object
      );
    }
    return false;
  });
}

/** Each string literal or template naming a command, as a matcher. */
function commandStrings(files: ts.SourceFile[]) {
  const sites: { matches: (name: string) => boolean; node: ts.Node }[] = [];
  for (const file of files) {
    walk(file, (node) => {
      if (
        ts.isStringLiteral(node) ||
        ts.isNoSubstitutionTemplateLiteral(node)
      ) {
        const text = node.text;
        sites.push({ matches: (name) => text === name, node });
      } else if (
        ts.isTemplateExpression(node) &&
        ts.isCallExpression(node.parent) &&
        node.parent.arguments[0] === node &&
        node.parent.expression.getText() === "invoke"
      ) {
        // invoke(`get_${app}_current_state`) and similar.
        const parts = [
          node.head.text,
          ...node.templateSpans.map((s) => s.literal.text),
        ];
        const pattern = new RegExp(
          `^${parts.map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("\\w+")}$`,
        );
        sites.push({ matches: (name) => pattern.test(name), node });
      }
    });
  }
  return sites;
}

describe("Tauri command registry", () => {
  it("registers only commands the frontend reaches through a used wrapper", () => {
    const files = frontendFiles(path.join(ROOT, "src"));
    const sites = commandStrings(files);
    const index = identifiersByName(files);
    const unreached = registeredCommands().flatMap((command) => {
      const wrappers = sites
        .filter((site) => site.matches(command))
        .map((site) => enclosingWrapper(site.node));
      const reached = wrappers.some(
        (wrapper) => wrapper === undefined || isReferenced(wrapper, index),
      );
      if (reached) return [];
      const via = wrappers.map(
        (w) => (w!.object ? `${w!.object}.` : "") + w!.name.text,
      );
      return [
        via.length ? `${command} (only via unused ${via.join(", ")})` : command,
      ];
    });
    expect(
      unreached,
      "These commands are registered in lib.rs generate_handler! but nothing under src/ reaches them: either no string names them, or every function naming them is itself never used. Delete the command (and its registration and wrapper), or call it from the frontend.",
    ).toEqual([]);
  });
});
