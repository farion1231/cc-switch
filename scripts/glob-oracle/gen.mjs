// 用与 pi 相同版本的 minimatch（默认选项，与 pi 的 matchesAnyPattern 调用一致）
// 生成 Rust glob 匹配器的 oracle fixture。
// 用法：pnpm install && pnpm gen
import { minimatch } from "minimatch";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// 全部为「支持语法」的 pattern —— 会经 minimatch 生成期望结果
const patterns = [
  "*", "**", "pdf", "SKILL.md",
  "*.md", "?.md", "*df", "pdf*",
  "skills/*", "skills/**", "skills/**/SKILL.md",
  "**/pdf", "**/SKILL.md", "**/*.md", "**/skills/**",
  "pdf/SKILL.md", "skills/pdf/SKILL.md",
  "a/**", "foo/**/bar", "foo/**/**/bar",
  "[ps]df", "[!p]df", "[a-z]*", "[a-z]?.md", "[]]",
  ".*", ".h*", "*.", "a.b", "[.]", "[.x]x", ".[x]", ".?", "[!.]x", "[.-0]x", "[]",
  "~/.claude/*", "~/.claude/skills", "skills/pdf*",
  "\\*literal", "a\\?b",
  // 坏范围 / identity escape / 零内容类（上游 review：不得 panic，
  // 语义对齐 minimatch makeRe：坏范围贡献为空、[\q] 是字面 q、[]/[!] 是字面量）
  "[z-a]", "[!z-a]", "[xz-a]", "[!xz-a]", "[b-a]", "[a-a]",
  "[a-c-e]", "[z-a-q]", "[a-c-z]", "[\\q]", "[\\q]b", "a[\\q]b",
  "[\\q-\\s]", "[!]", "[\\d]", "[a\\-b]",
];

const targets = [
  "", "pdf", "SKILL.md", "skills", "skills/pdf", "skills/pdf/SKILL.md",
  "a", "a/b", "a/b/c", "foo/x/y/bar", "foo/bar", "foo//bar",
  ".hidden", ".hidden.md", "pdf-tools", "pdf.md", "pdf/SKILL.md",
  "~/.claude/skills", "~/.claude/skills/pdf",
  "agents/skills/pdf/SKILL.md", "x/y/z/SKILL.md", "*literal", "a?b", "]",
  ".", "..", ".x", ".x/SKILL.md", "skills/.hidden",
  "q", "r", "s", "x", "z", "b", "d", "0", "-", "!", "[]", "[!]", "[z-a]",
];

const cases = [];
for (const pattern of patterns) {
  for (const target of targets) {
    cases.push({ pattern, target, matches: minimatch(target, pattern) });
  }
}

// 不支持语法的 pattern（花括号展开、extglob）—— 仅用于 is_supported_pattern 断言
const unsupported = ["{a,b}.md", "a{1,2}", "a+(b)", "a?(b)", "@(a|b)", "!(a)", "a*(", "a@("];

const out = { minimatchVersion: "10.2.6", cases, unsupported };
const here = dirname(fileURLToPath(import.meta.url));
const outPath = join(here, "../../src-tauri/src/services/skill/testdata/glob-oracle.json");
mkdirSync(dirname(outPath), { recursive: true });
writeFileSync(outPath, JSON.stringify(out, null, 2) + "\n");
console.log(`wrote ${cases.length} match cases (${patterns.length} patterns x ${targets.length} targets), ${unsupported.length} unsupported samples`);
