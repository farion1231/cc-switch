import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import zh from "@/i18n/locales/zh.json";
import { ApiKeySection } from "@/components/providers/forms/shared/ApiKeySection";

describe("TokenPlus description", () => {
  it("renders the complete Chinese introduction below the key input", async () => {
    const i18n = createInstance();
    await i18n.init({ lng: "zh", resources: { zh: { translation: zh } } });
    render(
      <I18nextProvider i18n={i18n}>
        <ApiKeySection
          value=""
          onChange={() => {}}
          category="aggregator"
          shouldShowLink
          websiteUrl="https://www.tokenplus.cn/workspace"
          partnerPromotionKey="tokenplus"
        />
      </I18nextProvider>,
    );
    expect(
      screen.getByText(zh.providerForm.partnerPromotion.tokenplus),
    ).toBeVisible();
  });
});
