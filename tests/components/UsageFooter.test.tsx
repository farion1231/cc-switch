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
import type { Provider } from "@/types";
import zh from "@/i18n/locales/zh.json";
import en from "@/i18n/locales/en.json";

const i18n = createInstance();

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "zh",
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
    interpolation: { escapeValue: false },
  });
});

beforeEach(async () => {
  await i18n.changeLanguage("zh");
});

afterEach(() => vi.restoreAllMocks());

// Mock useSettings
const useSettingsMock = vi.fn();
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => useSettingsMock(),
}));

// Mock useUsageQuery
const useUsageQueryMock = vi.fn();
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: (...args: unknown[]) => useUsageQueryMock(...args),
}));

function createProvider(): Provider {
  return {
    id: "test-provider",
    name: "Test Provider",
    settingsConfig: {},
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "",
        autoQueryInterval: 0,
      },
    },
  };
}

describe("UsageFooter - usageDisplayMode", () => {
  it("shows used percentage when mode is 'used' with total", () => {
    useSettingsMock.mockReturnValue({
      settings: { usageDisplayMode: "used" },
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", total: 100, used: 30, remaining: 70 }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 应该显示 "已使用 30%"
    expect(screen.getByText(/已使用 30%/)).toBeInTheDocument();
  });

  it("shows used value when mode is 'used' without total", () => {
    useSettingsMock.mockReturnValue({
      settings: { usageDisplayMode: "used" },
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", used: 30, unit: "USD" }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 应该显示 "已使用： 30.00 USD"
    expect(screen.getByText(/已使用/)).toBeInTheDocument();
    expect(screen.getByText(/30.00/)).toBeInTheDocument();
  });

  it("shows both used and remaining when mode is 'both' with total", () => {
    useSettingsMock.mockReturnValue({
      settings: { usageDisplayMode: "both" },
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", total: 100, used: 30, remaining: 70 }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 应该同时显示 "已使用 30%" 和 "剩余 70%"
    expect(screen.getByText(/已使用 30%/)).toBeInTheDocument();
    expect(screen.getByText(/剩余 70%/)).toBeInTheDocument();
  });

  it("shows both values when mode is 'both' without total", () => {
    useSettingsMock.mockReturnValue({
      settings: { usageDisplayMode: "both" },
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", used: 30, remaining: 70, unit: "USD" }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 应该同时显示 "已用 30.00 USD" 和 "剩余 70.00 USD"
    expect(screen.getByText(/已用 30.00 USD/)).toBeInTheDocument();
    expect(screen.getByText(/剩余 70.00 USD/)).toBeInTheDocument();
  });

  it("shows remaining by default (backward compatible)", () => {
    useSettingsMock.mockReturnValue({
      settings: {}, // 无 usageDisplayMode
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", total: 100, used: 30, remaining: 70, unit: "USD" }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 默认模式：显示余额（balanceLine 行为）
    expect(screen.getByText(/余额 70.00 USD/)).toBeInTheDocument();
  });

  it("shows used value when mode is 'remaining' but no remaining data", () => {
    useSettingsMock.mockReturnValue({
      settings: { usageDisplayMode: "remaining" },
    });
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [{ planName: "Test Plan", used: 30, unit: "USD" }],
      },
      isFetching: false,
      isError: false,
      lastQueriedAt: null,
      refetch: vi.fn(),
    });

    render(
      <I18nextProvider i18n={i18n}>
        <UsageFooter
          provider={createProvider()}
          providerId="test-provider"
          appId="claude"
          usageEnabled={true}
          isCurrent={true}
        />
      </I18nextProvider>,
    );

    // 无 remaining 时回退到显示已用
    expect(screen.getByText(/已使用/)).toBeInTheDocument();
    expect(screen.getByText(/30.00/)).toBeInTheDocument();
  });
});
