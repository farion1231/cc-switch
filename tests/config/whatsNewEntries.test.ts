import { describe, expect, it } from "vitest";
import {
  WHATS_NEW_ITEM_TYPES,
  WHATS_NEW_LANGUAGES,
  type WhatsNewLanguage,
} from "@/lib/whatsNew";

// 规则见 src/whats-new/README.md：弹窗只放几句话，详细内容去官网看
const MAX_ITEMS = 4;
const MAX_LENGTH: Record<WhatsNewLanguage, number> = {
  zh: 40,
  "zh-TW": 40,
  ja: 50,
  en: 100,
};

const files = import.meta.glob<unknown>("../../src/whats-new/*.json", {
  eager: true,
  import: "default",
});

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

// 弹窗渲染时直接读 item.type、item[语言]，结构不对会让整个应用落进错误页，
// 所以这里对任意 JSON 都只报问题、不抛错。发版前 release.yml 也跑这份检查。
function problemsOf(path: string, data: unknown): string[] {
  const name = path.split("/").pop()!;
  const problems: string[] = [];

  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?\.json$/.test(name)) {
    problems.push(`${name}: file name must be <version>.json`);
  }
  if (!isObject(data)) {
    return [...problems, `${name}: must be a JSON object`];
  }
  const entry = data;
  if (`${String(entry.version)}.json` !== name) {
    problems.push(`${name}: "version" must match the file name`);
  }
  if (!Array.isArray(entry.items)) {
    return [...problems, `${name}: "items" must be an array`];
  }
  if (entry.items.length > MAX_ITEMS) {
    problems.push(`${name}: at most ${MAX_ITEMS} items`);
  }
  entry.items.forEach((item: unknown, index) => {
    const where = `${name} items[${index}]`;
    if (!isObject(item)) {
      problems.push(`${where}: must be an object`);
      return;
    }
    if (!(WHATS_NEW_ITEM_TYPES as readonly unknown[]).includes(item.type)) {
      problems.push(`${where}: unknown type ${String(item.type)}`);
    }
    for (const language of WHATS_NEW_LANGUAGES) {
      const text = item[language];
      if (typeof text !== "string" || text.trim() === "") {
        problems.push(`${where}: missing ${language}`);
      } else if ([...text].length > MAX_LENGTH[language]) {
        problems.push(
          `${where}: ${language} is longer than ${MAX_LENGTH[language]} characters`,
        );
      }
    }
  });
  return problems;
}

describe("what's new entries", () => {
  it("follow the format and stay short", () => {
    const problems = Object.entries(files).flatMap(([path, data]) =>
      problemsOf(path, data),
    );
    expect(problems).toEqual([]);
  });

  it("the checker catches broken entries", () => {
    expect(
      problemsOf("x/4.0.2.json", {
        version: "4.0.3",
        items: [{ type: "news", zh: "", en: "x".repeat(101) }],
      }),
    ).toEqual([
      '4.0.2.json: "version" must match the file name',
      "4.0.2.json items[0]: unknown type news",
      "4.0.2.json items[0]: missing zh",
      "4.0.2.json items[0]: missing zh-TW",
      "4.0.2.json items[0]: en is longer than 100 characters",
      "4.0.2.json items[0]: missing ja",
    ]);
  });

  it("reports malformed JSON shapes instead of throwing", () => {
    expect(
      problemsOf("x/4.0.2.json", { version: "4.0.2", items: [null] }),
    ).toEqual(["4.0.2.json items[0]: must be an object"]);
    expect(problemsOf("x/4.0.2.json", null)).toEqual([
      "4.0.2.json: must be a JSON object",
    ]);
  });
});
