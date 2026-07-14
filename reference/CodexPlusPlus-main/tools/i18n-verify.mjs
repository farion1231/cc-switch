// Verifies the English dictionary (src/i18n-en.ts) covers exactly the keys that
// are actually referenced by t("…") / tf("…") calls in the frontend source — no
// missing keys, no stale extras. Scanning real call sites (rather than trusting
// tools/i18n-keys.json) means hand-added wrapped strings are validated too, not
// just the codemod's output.
//
// Run after editing the dictionary, adding a t()/tf() call, or re-running the
// codemod:
//   node tools/i18n-verify.mjs

import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const appRoot = path.join(repoRoot, "apps", "codex-plus-manager");
const require = createRequire(path.join(appRoot, "package.json"));
const ts = require("typescript");

const SRC_FILES = [
  "src/App.tsx",
  "src/components/ProviderPresetSelector.tsx",
];

// ── Collect the keys referenced by t()/tf() across the source. ──────────────
const usedPlain = new Set();
const usedTemplate = new Set();

function scanFile(relPath) {
  const abs = path.join(appRoot, relPath);
  const text = readFileSync(abs, "utf8");
  const sf = ts.createSourceFile(abs, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

  const visit = (node) => {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
      const fn = node.expression.text;
      const arg = node.arguments[0];
      const literal =
        arg && (ts.isStringLiteral(arg) || ts.isNoSubstitutionTemplateLiteral(arg)) ? arg.text : null;
      if (literal !== null) {
        if (fn === "t") usedPlain.add(literal);
        else if (fn === "tf") usedTemplate.add(literal);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
}

for (const f of SRC_FILES) scanFile(f);

// ── Read the dictionary keys out of i18n-en.ts. ─────────────────────────────
const dictSource = readFileSync(path.join(appRoot, "src", "i18n-en.ts"), "utf8");

function recordKeys(varName) {
  const re = new RegExp(`export const ${varName}\\s*:\\s*Record<string,\\s*string>\\s*=\\s*\\{`);
  const match = re.exec(dictSource);
  if (!match) throw new Error(`Could not find ${varName} in i18n-en.ts`);
  const start = match.index + match[0].length - 1; // position of "{"
  let depth = 0;
  let i = start;
  let inStr = null;
  let escaped = false;
  for (; i < dictSource.length; i++) {
    const ch = dictSource[i];
    if (inStr) {
      if (escaped) escaped = false;
      else if (ch === "\\") escaped = true;
      else if (ch === inStr) inStr = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") inStr = ch;
    else if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) { i++; break; }
    }
  }
  const literal = dictSource.slice(start, i);
  // eslint-disable-next-line no-eval
  const obj = (0, eval)(`(${literal})`);
  return new Set(Object.keys(obj));
}

let ok = true;

function check(label, usedSet, dictSet) {
  const missing = [...usedSet].filter((k) => !dictSet.has(k));
  const extra = [...dictSet].filter((k) => !usedSet.has(k));
  console.log(`${label}: ${usedSet.size} referenced, ${dictSet.size} translated`);
  if (missing.length) {
    ok = false;
    console.log(`  MISSING from dictionary (${missing.length}):`);
    for (const k of missing) console.log(`    ${JSON.stringify(k)}`);
  }
  if (extra.length) {
    ok = false;
    console.log(`  STALE in dictionary (${extra.length}):`);
    for (const k of extra) console.log(`    ${JSON.stringify(k)}`);
  }
}

check("plain", usedPlain, recordKeys("EN_PLAIN"));
check("template", usedTemplate, recordKeys("EN_TEMPLATE"));

if (!ok) {
  console.error("\nDictionary does not match t()/tf() usage in source.");
  process.exit(1);
}
console.log("\nDictionary matches every t()/tf() call site exactly.");
