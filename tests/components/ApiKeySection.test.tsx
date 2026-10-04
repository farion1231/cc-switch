import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ApiKeySection } from "@/components/providers/forms/shared/ApiKeySection";

// 测试的 i18n 是空资源，推广语传了 defaultValue: "" 会变成空串；这里让 t() 原样返回 key
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function renderSection(
  props: Partial<React.ComponentProps<typeof ApiKeySection>> = {},
) {
  return render(
    <ApiKeySection
      value=""
      onChange={() => {}}
      category="third_party"
      shouldShowLink
      websiteUrl="https://example.com/keys"
      partnerPromotionKey="kimi"
      {...props}
    />,
  );
}

describe("ApiKeySection", () => {
  it("shows the preset's promotion under the input next to the API Key link", () => {
    renderSection();

    expect(
      screen.getByRole("link", { name: /providerForm\.getApiKey/ }),
    ).toHaveAttribute("href", "https://example.com/keys");
    expect(
      screen.getByText("providerForm.partnerPromotion.kimi"),
    ).toBeInTheDocument();
  });

  it("hides the promotion when the API Key link is hidden", () => {
    renderSection({ category: "official", shouldShowLink: false });

    expect(
      screen.queryByText("providerForm.partnerPromotion.kimi"),
    ).not.toBeInTheDocument();
  });

  it("renders nothing extra for presets without a promotion", () => {
    const { container } = renderSection({ partnerPromotionKey: undefined });

    expect(container.querySelector("p")).toBeNull();
  });
});
