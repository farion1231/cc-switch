import i18n from "@/i18n";
import { providerSchema } from "@/lib/schemas/provider";
import { afterEach, describe, expect, it } from "vitest";

type Issue = {
  path: PropertyKey[];
  message: string;
};

function getIssueMessage(issues: Issue[], field: string): string {
  return issues.find((issue) => issue.path[0] === field)?.message ?? "";
}

describe("providerSchema", () => {
  afterEach(async () => {
    await i18n.changeLanguage("zh");
  });

  it("recomputes validation messages when the active language changes", async () => {
    await i18n.changeLanguage("en");

    const englishResult = providerSchema.safeParse({
      name: "Demo",
      websiteUrl: "not-a-url",
      settingsConfig: "",
    });

    expect(englishResult.success).toBe(false);
    if (!englishResult.success) {
      expect(getIssueMessage(englishResult.error.issues, "websiteUrl")).toBe(
        "Please enter a valid URL",
      );
      expect(
        getIssueMessage(englishResult.error.issues, "settingsConfig"),
      ).toBe("Please fill in the configuration");
    }

    await i18n.changeLanguage("ru");

    const russianResult = providerSchema.safeParse({
      name: "Demo",
      websiteUrl: "not-a-url",
      settingsConfig: "",
    });

    expect(russianResult.success).toBe(false);
    if (!russianResult.success) {
      expect(getIssueMessage(russianResult.error.issues, "websiteUrl")).toBe(
        "Введите действительный URL",
      );
      expect(
        getIssueMessage(russianResult.error.issues, "settingsConfig"),
      ).toBe("Укажите содержимое конфигурации");
    }
  });

  it("keeps JSON parse fallback text in the active locale", async () => {
    await i18n.changeLanguage("en");

    const englishResult = providerSchema.safeParse({
      name: "Demo",
      websiteUrl: "https://example.com",
      settingsConfig: "{ invalid",
    });

    expect(englishResult.success).toBe(false);
    if (!englishResult.success) {
      const message = getIssueMessage(
        englishResult.error.issues,
        "settingsConfig",
      );
      expect(message).toMatch(/^Invalid JSON format:/);
      expect(message).not.toContain("意外的");
      expect(message).not.toContain("符号");
      expect(message).not.toContain("预期");
    }

    await i18n.changeLanguage("zh");

    const chineseResult = providerSchema.safeParse({
      name: "Demo",
      websiteUrl: "https://example.com",
      settingsConfig: "{ invalid",
    });

    expect(chineseResult.success).toBe(false);
    if (!chineseResult.success) {
      expect(
        getIssueMessage(chineseResult.error.issues, "settingsConfig"),
      ).toContain("JSON 格式错误：");
    }
  });
});
