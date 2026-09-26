import { render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import UsageFooter from "@/components/UsageFooter";
import { TEMPLATE_TYPES } from "@/config/constants";
import type { TemplateType } from "@/config/constants";
import type { Provider } from "@/types";
import zh from "@/i18n/locales/zh.json";

const fixtures = vi.hoisted(() => ({
  queriedAt: Date.parse("2026-09-15T09:54:00Z"),
  resetsAt: "2026-09-15T12:46:00Z",
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({
    data: {
      success: true,
      data: [
        {
          planName: "monthly",
          used: 4,
          total: 100,
          remaining: 96,
          unit: "%",
          // A bare ISO timestamp, exactly what the backend puts here for
          // reset-bearing templates — and what a custom script may put here
          // meaning something else entirely.
          extra: fixtures.resetsAt,
        },
      ],
    },
    isFetching: false,
    isError: false,
    lastQueriedAt: fixtures.queriedAt,
    refetch: () => {},
  }),
}));

const i18n = createInstance();

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "zh",
    resources: { zh: { translation: zh } },
    interpolation: { escapeValue: false },
  });
});

beforeEach(() => vi.spyOn(Date, "now").mockReturnValue(fixtures.queriedAt));
afterEach(() => vi.restoreAllMocks());

/** The local-clock label the UI promises for an instant, whatever the runner TZ. */
function expectedLocalClock(iso: string): string {
  const at = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

function renderFooter(templateType: TemplateType) {
  const provider: Provider = {
    id: "p1",
    name: "Relay",
    settingsConfig: { auth: { OPENAI_API_KEY: "sk-test" }, config: "" },
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "",
        templateType,
      },
    },
  };
  return render(
    <I18nextProvider i18n={i18n}>
      <UsageFooter
        provider={provider}
        providerId="p1"
        appId="codex"
        usageEnabled
        isCurrent
        inline
      />
    </I18nextProvider>,
  );
}

describe("UsageFooter extra rendering", () => {
  it("leaves an ISO-shaped extra untouched for a custom script", () => {
    // `extra` is documented as free-form display text, so this could be an
    // expiry or a billing date. Relabelling it as a quota reset would be wrong.
    renderFooter(TEMPLATE_TYPES.CUSTOM);
    expect(screen.getByText(fixtures.resetsAt)).toBeInTheDocument();
    expect(screen.queryByText(/后重置/)).not.toBeInTheDocument();
  });

  it("reads an ISO extra as a reset moment for a reset-bearing template", () => {
    renderFooter(TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION);
    expect(
      screen.getByText(
        `2h52m后重置（${expectedLocalClock(fixtures.resetsAt)}）`,
      ),
    ).toBeInTheDocument();
  });
});
