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

describe("ApiKeySection variable references", () => {
  it("shows the reference as plain text with a status line", () => {
    renderSection({ value: "{{MY_KEY}}" });
    const input = screen.getByDisplayValue("{{MY_KEY}}");
    expect(input).toHaveAttribute("type", "text");
    // 没有 QueryClientProvider 时变量列表为空：显示「未定义」
    expect(screen.getByTestId("api-key-ref-status")).toHaveTextContent(
      "apiKeyVars.ref.unknown",
    );
  });

  it("does not show reference hints when references are not allowed", () => {
    renderSection({ value: "{{MY_KEY}}", allowVarRef: false });
    expect(screen.queryByTestId("api-key-ref-status")).toBeNull();
    expect(screen.getByDisplayValue("{{MY_KEY}}")).toHaveAttribute(
      "type",
      "password",
    );
  });

  it("keeps a disabled API Key input free of variable-reference UI", () => {
    renderSection({ value: "{{MY_KEY}}", disabled: true });

    expect(screen.queryByTestId("api-key-ref-status")).toBeNull();
    expect(screen.getByDisplayValue("{{MY_KEY}}")).toBeDisabled();
    expect(screen.getByDisplayValue("{{MY_KEY}}")).toHaveAttribute(
      "type",
      "password",
    );
  });
});
