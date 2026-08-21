import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createInstance, type i18n } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import type { ReactNode } from "react";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import { CopilotCliEnvironmentDialog } from "@/components/settings/CopilotCliEnvironmentDialog";
import { CopilotByokGroupPanel } from "@/components/settings/CopilotByokGroupPanel";
import { CopilotByokSettings } from "@/components/settings/CopilotByokSettings";
import { CopilotErrorMessage } from "@/components/settings/CopilotErrorMessage";
import { CopilotImportWarnings } from "@/components/settings/CopilotImportWarnings";
import { COPILOT_ERROR_KEYS, copilotUiError } from "@/lib/copilotByokMessages";
import type {
  CopilotByokGroup,
  CopilotByokState,
  CopilotByokTargetState,
  CopilotCliEnvironmentPreview,
} from "@/lib/api/copilotByok";

const mocks = vi.hoisted(() => ({
  getState: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  copilotByokApi: { getState: mocks.getState },
  copilotCliApi: { getState: mocks.getState },
  settingsApi: { pickDirectory: vi.fn() },
}));
vi.mock("@/lib/modelsDev", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/modelsDev")>();
  return {
    ...actual,
    modelsDevQueryOptions: {
      ...actual.modelsDevQueryOptions,
      queryFn: vi.fn().mockResolvedValue({}),
    },
  };
});
vi.mock("sonner", () => ({
  toast: {
    error: mocks.toastError,
    success: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

const resources = {
  en: { translation: en },
  zh: { translation: zh },
  "zh-TW": { translation: zhTW },
  ja: { translation: ja },
};
const languages = ["zh", "zh-TW", "en", "ja"] as const;

async function translations(language: string) {
  const engine = createInstance();
  await engine.use(initReactI18next).init({
    resources,
    lng: language,
    fallbackLng: false,
    interpolation: { escapeValue: false },
  });
  return engine;
}

function renderTranslated(engine: i18n, children: ReactNode) {
  return render(<I18nextProvider i18n={engine}>{children}</I18nextProvider>);
}

const group: CopilotByokGroup = {
  id: "test-provider",
  name: "User provider",
  url: "https://example.com/v1/responses",
  apiKey: "",
  apiType: "responses",
  enabled: true,
  requestHeaders: {},
  extra: {},
  models: [
    {
      id: "test-model",
      modelId: "custom-model",
      name: "User model",
      enabled: true,
      editTools: [],
      zeroDataRetentionEnabled: false,
      supportsReasoningEffort: [],
      reasoningEffortFormat: null,
      modelOptions: {},
      extra: {},
    },
  ],
};
const target: CopilotByokTargetState = {
  id: "unnamed-target",
  source: "custom",
  edition: null,
  editionName: null,
  profileId: null,
  profileName: "",
  isDefault: false,
  languageModelsPath: "C:/test/chatLanguageModels.json",
  configExists: false,
  backupExists: false,
  selected: false,
  managedGroupCount: 0,
  readError: null,
};
const state: CopilotByokState = {
  groups: [],
  targets: [
    target,
    { ...target, id: "named-target", profileName: "Custom VS Code profile" },
  ],
  selectedTargetIds: [],
  managedModelCount: 0,
  cli: {
    supported: true,
    enabled: false,
    selectedGroupId: null,
    selectedModelId: null,
    selectedProviderName: null,
    selectedModelName: null,
    environmentMatches: true,
    environmentConflicts: [],
    officialActivationRequiresConfirmation: false,
  },
};
const preview: CopilotCliEnvironmentPreview = {
  groupId: group.id,
  confirmationToken: "test-token",
  integrationConflicts: ["/test/.bashrc"],
  changes: [
    {
      name: "COPILOT_PROVIDER_API_KEY",
      source: "HKEY_CURRENT_USER\\Environment",
      sensitive: true,
      conflict: true,
      previous: { isSet: true, value: null },
      current: { isSet: false, value: null },
      desired: { isSet: true, value: null },
    },
  ],
};

describe.each(languages)("Copilot UI with real %s translations", (language) => {
  beforeEach(() => {
    mocks.getState.mockReset().mockResolvedValue(state);
    mocks.toastError.mockClear();
  });

  it("translates preview labels and interpolates a long provider name", async () => {
    const engine = await translations(language);
    const providerName = "User-defined-".repeat(18);
    renderTranslated(
      engine,
      <CopilotCliEnvironmentDialog
        preview={preview}
        providerName={providerName}
        pending={false}
        error={null}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(
      screen.getByText(engine.t("copilotByok.cli.previewTitle")),
    ).toBeInTheDocument();
    for (const key of ["previous", "current", "desired"]) {
      expect(
        screen.getByText(engine.t(`copilotByok.cli.${key}Value`)),
      ).toBeInTheDocument();
    }
    expect(
      screen.getAllByText(engine.t("copilotByok.cli.valueHidden")),
    ).toHaveLength(2);
    const confirm = screen.getByRole("button", {
      name: engine.t("copilotByok.cli.confirmReapply", {
        provider: providerName,
      }),
    });
    expect(confirm).toHaveClass("whitespace-normal");
    expect(document.body.textContent).not.toMatch(/copilotByok\.|\{\{/);
  });

  it("localizes every operation error and keeps diagnostics in closed details", async () => {
    const engine = await translations(language);
    const diagnostic = "Untranslated operating-system diagnostic";
    renderTranslated(
      engine,
      <>
        {Object.entries(COPILOT_ERROR_KEYS).map(([operation, messageKey]) => (
          <CopilotErrorMessage
            key={operation}
            error={{ messageKey, details: diagnostic }}
          />
        ))}
      </>,
    );
    for (const key of Object.values(COPILOT_ERROR_KEYS)) {
      expect(screen.getByText(engine.t(key))).toBeInTheDocument();
    }
    expect(screen.getAllByText(engine.t("copilotByok.details"))).toHaveLength(
      Object.keys(COPILOT_ERROR_KEYS).length,
    );
    expect(
      [...document.querySelectorAll("details")].every(
        (details) => !details.open,
      ),
    ).toBe(true);
  });

  it("localizes import warning codes, including unnamed and legacy warnings", async () => {
    const engine = await translations(language);
    renderTranslated(
      engine,
      <CopilotImportWarnings
        warnings={[
          { code: "secretReference", groupName: "User provider" },
          {
            code: "skippedGroup",
            groupName: null,
            groupIndex: 2,
            detail: "Raw parse diagnostic",
          },
          "Legacy warning diagnostic",
        ]}
      />,
    );
    expect(
      screen.getByText(
        engine.t("copilotByok.warnings.secretReference", {
          provider: "User provider",
        }),
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        engine.t("copilotByok.warnings.skippedGroup", {
          provider: engine.t("copilotByok.warnings.unnamedGroup", { index: 2 }),
        }),
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(engine.t("copilotByok.warnings.legacy")),
    ).toBeInTheDocument();
    expect(
      [...document.querySelectorAll("details")].every(
        (details) => !details.open,
      ),
    ).toBe(true);
  });

  it("translates an unnamed target without translating an explicit user name", async () => {
    const engine = await translations(language);
    renderTranslated(engine, <CopilotByokSettings mode="targets" />);
    expect(
      await screen.findByRole("checkbox", {
        name: engine.t("copilotByok.customTarget"),
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "Custom VS Code profile" }),
    ).toBeInTheDocument();
  });

  it("uses a localized title for a failed backend operation", async () => {
    const engine = await translations(language);
    mocks.getState.mockRejectedValue(new Error("Raw backend diagnostic"));
    renderTranslated(engine, <CopilotByokSettings mode="targets" />);
    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith(
        engine.t(COPILOT_ERROR_KEYS.load),
        expect.objectContaining({ description: expect.anything() }),
      ),
    );
  });

  it.each(["{ malformed", "[]", "null", "123"])(
    "translates invalid model-option JSON %s",
    async (invalidJson) => {
      const engine = await translations(language);
      const onSave = vi.fn();
      renderTranslated(
        engine,
        <CopilotByokGroupPanel
          open
          group={group}
          saving={false}
          onOpenChange={vi.fn()}
          onSave={onSave}
        />,
      );
      fireEvent.click(
        screen.getByRole("button", {
          name: engine.t("opencode.toggleModelDetails"),
        }),
      );
      fireEvent.change(document.querySelector("textarea")!, {
        target: { value: invalidJson },
      });
      fireEvent.submit(document.querySelector("form")!);
      expect(
        await screen.findByText(
          engine.t(COPILOT_ERROR_KEYS.invalidModelOptions),
        ),
      ).toBeInTheDocument();
      expect(onSave).not.toHaveBeenCalled();
      expect(document.body.textContent).not.toMatch(
        /SyntaxError|Unexpected token/,
      );
    },
  );

  it("localizes duplicate model IDs before attempting to save", async () => {
    const engine = await translations(language);
    const onSave = vi.fn();
    renderTranslated(
      engine,
      <CopilotByokGroupPanel
        open
        group={{
          ...group,
          models: [group.models[0], { ...group.models[0], id: "other-model" }],
        }}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );
    fireEvent.submit(document.querySelector("form")!);
    expect(
      await screen.findByText(engine.t(COPILOT_ERROR_KEYS.duplicateModelId)),
    ).toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
  });

  it("localizes a provider-form save failure and preserves its diagnostic", async () => {
    const engine = await translations(language);
    const onSave = vi
      .fn()
      .mockRejectedValue(new Error("File permission diagnostic"));
    renderTranslated(
      engine,
      <CopilotByokGroupPanel
        open
        group={group}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );
    fireEvent.submit(document.querySelector("form")!);
    expect(
      await screen.findByText(engine.t(COPILOT_ERROR_KEYS.save)),
    ).toBeInTheDocument();
    expect(screen.getByText("File permission diagnostic")).toBeInTheDocument();
    expect(document.querySelector("details")?.open).toBe(false);
  });
});

it("updates an already visible error and its details label when the language changes", async () => {
  const engine = await translations("en");
  const error = copilotUiError("apply", new Error("Preserved diagnostic"));
  renderTranslated(engine, <CopilotErrorMessage error={error} />);
  expect(screen.getByText(en.copilotByok.errors.apply)).toBeInTheDocument();
  await act(async () => {
    await engine.changeLanguage("ja");
  });
  expect(screen.getByText(ja.copilotByok.errors.apply)).toBeInTheDocument();
  expect(screen.getByText(ja.copilotByok.details)).toBeInTheDocument();
  expect(screen.getByText("Preserved diagnostic")).toBeInTheDocument();
  expect(
    screen.queryByText(en.copilotByok.errors.apply),
  ).not.toBeInTheDocument();
});
