import { render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { CodexSwitchReview } from "@/components/providers/CodexSwitchReview";
import { CodexImportFields } from "@/components/deeplink/CodexImportFields";
import { codexImportError } from "@/utils/codexImport";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

describe("Codex customization translations", () => {
  it.each([
    ["zh", zh],
    ["en", en],
    ["ja", ja],
    ["zh-TW", zhTW],
  ] as const)(
    "renders import fields, backend field labels and validation in %s",
    async (language, dictionary) => {
      const i18n = createInstance();
      await i18n.init({
        lng: language,
        resources: { [language]: { translation: dictionary } },
      });
      const strings = dictionary.codexCustomization;
      const request: DeepLinkImportRequest = {
        version: "v1",
        resource: "provider",
        app: "codex",
        modelCatalog: { models: [] },
      };
      const { unmount } = render(
        <I18nextProvider i18n={i18n}>
          <CodexImportFields request={request} onChange={vi.fn()} />
        </I18nextProvider>,
      );
      expect(screen.getByText(strings.import.title)).toBeInTheDocument();
      expect(
        screen.getByLabelText(strings.import.endpoint),
      ).toBeInTheDocument();
      expect(i18n.t(codexImportError(request)!)).toBe(strings.error.endpoint);
      unmount();
      render(
        <I18nextProvider i18n={i18n}>
          <CodexSwitchReview
            onClose={vi.fn()}
            preview={{
              revision: "snapshot",
              proxy: false,
              fields: [
                {
                  id: "connection",
                  label: "codexCustomization.review.connection",
                  before: "",
                  after: "",
                  selected: true,
                },
              ],
              models: [],
              currentModels: [],
              currentModel: null,
              defaultModel: null,
              currentEffort: null,
            }}
          />
        </I18nextProvider>,
      );
      expect(
        screen.getByRole("dialog", { name: strings.review.title }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("checkbox", { name: strings.review.connection }),
      ).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent(
        strings.review.invalid,
      );
      expect(
        screen.getByRole("button", { name: strings.review.apply }),
      ).toBeDisabled();
    },
  );
});
