import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// README 是整页文档，没有逐句回退：哪份漏改了，读者就看到一份半新半旧的页面。
// 这里拦住最容易漏的两类改动：章节结构和赞助商表。
const ROOT = path.resolve(__dirname, "../..");

const READMES = [
  ["README.md", "English"],
  ["README_ZH.md", "简体中文"],
  ["README_ZH-TW.md", "繁體中文"],
  ["README_JA.md", "日本語"],
  ["README_KO.md", "한국어"],
  ["README_TR.md", "Türkçe"],
  ["README_RU.md", "Русский"],
  ["README_DE.md", "Deutsch"],
] as const;

// 简体中文版面向大陆读者，赞助商表有地区版本（国内活动页、中文横幅），
// 只校验行数；其余各版与英文版逐个链接对齐。
const REGIONAL_SPONSOR_VARIANTS = new Set(["README_ZH.md"]);

const read = (file: string) => readFileSync(path.join(ROOT, file), "utf8");

const headingLevels = (text: string) =>
  [...text.matchAll(/^(#{1,4}) /gm)].map((m) => m[1].length);

const sponsorSection = (text: string) => {
  const start = text.search(/^## /m);
  const next = text.slice(start + 3).search(/^## /m);
  return text.slice(start, start + 3 + next);
};

const htmlLinks = (text: string) =>
  [...text.matchAll(/(?:href|src)="([^"]+)"/g)].map((m) => m[1]);

const sponsorRows = (text: string) =>
  (sponsorSection(text).match(/<tr>/g) ?? []).length;

const english = read("README.md");

describe.each(READMES)("%s", (file) => {
  it("exists", () => {
    expect(existsSync(path.join(ROOT, file))).toBe(true);
  });

  const text = existsSync(path.join(ROOT, file)) ? read(file) : "";

  it("links every other README from its language switcher", () => {
    const switcher = text
      .split("\n")
      .find((line) => line.includes("(CHANGELOG.md)") && line.includes("README"));
    expect(switcher, "language switcher line").toBeDefined();
    for (const [other, name] of READMES) {
      if (other === file) {
        expect(switcher).toContain(name);
        expect(switcher).not.toContain(`(${other})`);
      } else {
        expect(switcher).toContain(`[${name}](${other})`);
      }
    }
  });

  it("keeps the same heading structure as the English README", () => {
    expect(headingLevels(text)).toEqual(headingLevels(english));
  });

  it("keeps the same number of sponsor rows as the English README", () => {
    expect(sponsorRows(text)).toBe(sponsorRows(english));
  });

  if (!REGIONAL_SPONSOR_VARIANTS.has(file)) {
    it("keeps every HTML link and image in the same order as the English README", () => {
      expect(htmlLinks(text)).toEqual(htmlLinks(english));
    });
  }
});
