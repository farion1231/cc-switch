import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

describe("provider-switch completion guidance", () => {
  it("always tells the owner to restart Codex and create a new conversation", () => {
    expect(en.deeplink.providerSwitch.successDescription).toMatch(
      /restart Codex.*new conversation/i,
    );
    expect(zh.deeplink.providerSwitch.successDescription).toMatch(
      /重新启动 Codex.*新建对话/,
    );
    expect(zhTW.deeplink.providerSwitch.successDescription).toMatch(
      /重新啟動 Codex.*建立新對話/,
    );
    expect(ja.deeplink.providerSwitch.successDescription).toMatch(
      /Codex.*再起動.*新しい会話/,
    );
  });
});
