// i18n codemod for codex-plus-manager.
//
// Wraps every Chinese (CJK) UI string in the React frontend with a translation
// helper:
//   - plain string / no-substitution template  -> t("中文")
//   - template literal with ${...}              -> tf("前缀 {0} 后缀", [expr0, ...])
//   - JSX text                                  -> {t("中文")} (whitespace preserved)
// JSX attribute string values are wrapped in braces: title={t("...")}.
//
// Comments are never touched (they are not runtime-visible and need no toggle).
//
// Edits never overlap: a node that gets wrapped is not descended into. Chinese
// nested inside a template's ${...} interpolation is still translated, because
// each interpolated expression is recursively transformed when the tf() call is
// built (see transform()).
//
// Usage:
//   node tools/i18n-codemod.mjs            # dry run: writes tools/i18n-keys.json
//   node tools/i18n-codemod.mjs --write    # apply edits in place
//
// The script loads TypeScript from the manager app's node_modules.

import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const appRoot = path.join(repoRoot, "apps", "codex-plus-manager");
const require = createRequire(path.join(appRoot, "package.json"));
const ts = require("typescript");

const WRITE = process.argv.includes("--write");

// model-windows.ts is intentionally excluded: its only Chinese string is a
// test-only error message, and model-windows.test.ts imports the module in
// isolation under `node --test`, where an "@/i18n" dependency would not resolve.
const FILES = [
  "src/App.tsx",
  "src/components/ProviderPresetSelector.tsx",
];

const CJK = /[㐀-䶿一-鿿　-〿＀-￯]/;
const hasCjk = (s) => CJK.test(s);

// Collected dictionary keys across all files.
const plainKeys = new Set();
const templateKeys = new Set();

function isPropertyNamePosition(node) {
  const p = node.parent;
  if (!p) return false;
  if (ts.isPropertyAssignment(p) && p.name === node) return true;
  if (ts.isComputedPropertyName(p)) return true;
  if (ts.isImportDeclaration(p) || ts.isExportDeclaration(p)) return true;
  if (ts.isImportSpecifier(p) || ts.isExportSpecifier(p)) return true;
  if (ts.isModuleDeclaration(p)) return true;
  return false;
}

function isAlreadyWrapped(node) {
  const p = node.parent;
  if (p && ts.isCallExpression(p) && ts.isIdentifier(p.expression)) {
    if (p.expression.text === "t" || p.expression.text === "tf") return true;
  }
  return false;
}

function inJsxAttribute(node) {
  const p = node.parent;
  return p && ts.isJsxAttribute(p) && p.initializer === node;
}

// State for the file currently being processed.
let sourceText = "";
let needsImport = false;

/**
 * Collect non-overlapping wrapping edits within `node`'s subtree. Each edit is
 * { start, end, text } in absolute source offsets. A wrapped node is never
 * descended into; template interpolations are handled via recursive transform.
 */
function collect(node, edits) {
  if (ts.isJsxText(node)) {
    const raw = node.getText();
    if (hasCjk(raw)) {
      const lead = raw.match(/^\s*/)[0];
      const trail = raw.match(/\s*$/)[0];
      const core = raw.slice(lead.length, raw.length - trail.length);
      if (core && hasCjk(core)) {
        plainKeys.add(core);
        edits.push({
          start: node.getStart(),
          end: node.getEnd(),
          text: `${lead}{t(${JSON.stringify(core)})}${trail}`,
        });
        needsImport = true;
      }
    }
    return;
  }

  if ((ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) && hasCjk(node.text)) {
    if (!isPropertyNamePosition(node) && !isAlreadyWrapped(node)) {
      plainKeys.add(node.text);
      const call = `t(${JSON.stringify(node.text)})`;
      edits.push({
        start: node.getStart(),
        end: node.getEnd(),
        text: inJsxAttribute(node) ? `{${call}}` : call,
      });
      needsImport = true;
    }
    return; // string literals carry no CJK-bearing children
  }

  if (ts.isTemplateExpression(node) && hasCjk(node.getText()) && !isAlreadyWrapped(node)) {
    let key = node.head.text;
    const args = [];
    node.templateSpans.forEach((span, i) => {
      key += `{${i}}`;
      key += span.literal.text;
      // Recursively transform the interpolated expression so nested CJK (e.g.
      // a ternary with Chinese branches) is translated inside the tf() arg.
      args.push(transform(span.expression));
    });
    templateKeys.add(key);
    const call = `tf(${JSON.stringify(key)}, [${args.join(", ")}])`;
    edits.push({
      start: node.getStart(),
      end: node.getEnd(),
      text: inJsxAttribute(node) ? `{${call}}` : call,
    });
    needsImport = true;
    return; // interpolations already handled recursively above
  }

  ts.forEachChild(node, (child) => collect(child, edits));
}

/** Return the fully-transformed source text for a single subtree. */
function transform(node) {
  const start = node.getStart();
  const end = node.getEnd();
  const edits = [];
  collect(node, edits);
  edits.sort((a, b) => b.start - a.start);
  let out = sourceText.slice(start, end);
  for (const e of edits) {
    out = out.slice(0, e.start - start) + e.text + out.slice(e.end - start);
  }
  return out;
}

function processFile(relPath) {
  const abs = path.join(appRoot, relPath);
  sourceText = readFileSync(abs, "utf8");
  needsImport = false;
  const sf = ts.createSourceFile(abs, sourceText, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

  const edits = [];
  collect(sf, edits);

  if (!WRITE) return edits.length;

  edits.sort((a, b) => b.start - a.start);
  let out = sourceText;
  for (const e of edits) {
    out = out.slice(0, e.start) + e.text + out.slice(e.end);
  }

  if (needsImport && !/from "@\/i18n"/.test(out)) {
    const importStmt = `import { t, tf } from "@/i18n";`;
    const lastImport = [...out.matchAll(/^import .*?;[ \t]*$/gms)].pop();
    if (lastImport) {
      const insertAt = lastImport.index + lastImport[0].length;
      out = out.slice(0, insertAt) + "\n" + importStmt + out.slice(insertAt);
    } else {
      out = importStmt + "\n" + out;
    }
  }

  writeFileSync(abs, out, "utf8");
  return edits.length;
}

let total = 0;
for (const f of FILES) {
  const n = processFile(f);
  total += n;
  console.log(`${f}: ${n} edits`);
}

const keysPath = path.join(__dirname, "i18n-keys.json");
writeFileSync(
  keysPath,
  JSON.stringify({ plain: [...plainKeys].sort(), template: [...templateKeys].sort() }, null, 2),
  "utf8",
);
console.log(`\nTotal edits: ${total}`);
console.log(`Plain keys: ${plainKeys.size}, Template keys: ${templateKeys.size}`);
console.log(`Keys written to ${keysPath}`);
