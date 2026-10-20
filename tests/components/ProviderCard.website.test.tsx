import type { ComponentProps } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import zh from "@/i18n/locales/zh.json";
import type { ManagedAuthStatus } from "@/lib/api";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/ProviderIcon", () => ({ ProviderIcon: () => null }));
vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: undefined }),
}));
vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: undefined }),
}));

const WEBSITE = "https://console.example.com/pricing?plan=pro#rates";
const API_URL = "https://api.example.com/v1";
const NOTE = "Check promotional rates before topping up";

const provider = (overrides: Partial<Provider> = {}): Provider => ({
  id: "website-provider",
  name: "Relay provider",
  category: "custom",
  websiteUrl: WEBSITE,
  notes: NOTE,
  settingsConfig: { env: { ANTHROPIC_BASE_URL: API_URL } },
  ...overrides,
});

type CardProps = ComponentProps<typeof ProviderCard>;

function renderCard(
  initialProvider: Provider,
  {
    authStatus,
    ...overrides
  }: Partial<Omit<CardProps, "provider">> & {
    authStatus?: ManagedAuthStatus;
  } = {},
) {
  const queryClient = createTestQueryClient();
  if (authStatus) {
    queryClient.setQueryData(
      ["managed-auth-status", "codex_oauth"],
      authStatus,
    );
  }
  const props: CardProps = {
    provider: initialProvider,
    appId: "claude",
    presentation: { chips: [], buttons: [] },
    isCurrent: false,
    onEdit: vi.fn(),
    onDelete: vi.fn(),
    onConfigureUsage: vi.fn(),
    onOpenWebsite: vi.fn(),
    ...overrides,
  };
  const card = (nextProvider: Provider) => (
    <QueryClientProvider client={queryClient}>
      <ProviderCard {...props} provider={nextProvider} />
    </QueryClientProvider>
  );
  const view = render(card(initialProvider));
  return {
    ...view,
    props,
    rerenderProvider: (nextProvider: Provider) =>
      view.rerender(card(nextProvider)),
  };
}

const websiteButton = (host = "console.example.com") =>
  screen.getByRole("button", { name: `访问官网 · ${host}` });

describe("ProviderCard website shortcut", () => {
  beforeEach(() => {
    // The shared setup intentionally uses empty locale bundles. Load only the
    // shipped label under test; unrelated controls keep their existing labels.
    i18n.addResourceBundle("zh", "translation", {
      providerCard: {
        action: { visitWebsite: zh.providerCard.action.visitWebsite },
      },
    });
  });

  afterEach(() => {
    i18n.removeResourceBundle("zh", "translation");
  });

  it("keeps the manual note and opens the website rather than the API endpoint", async () => {
    const user = userEvent.setup();
    const { props } = renderCard(provider());

    expect(screen.getByText(NOTE)).toBeInTheDocument();
    await user.click(websiteButton());
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
    expect(props.onOpenWebsite).toHaveBeenCalledWith(WEBSITE);
  });

  it.each([
    [
      "https://Console.Example.COM:8443/a%2Fb?from=%2Fhome&plan=pro#rates",
      "console.example.com:8443",
    ],
    [
      "http://console.example.com/pricing?plan=pro#rates",
      "console.example.com",
    ],
  ])("trims but does not rewrite a valid website: %s", async (url, host) => {
    const user = userEvent.setup();
    const { props } = renderCard(provider({ websiteUrl: `  ${url}  ` }));

    await user.click(websiteButton(host));

    expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
    expect(props.onOpenWebsite).toHaveBeenCalledWith(url);
  });

  it.each([undefined, "", "   "])(
    "retains the existing website text link when the note is %j",
    async (notes) => {
      const user = userEvent.setup();
      const { props } = renderCard(provider({ notes }));

      await user.click(screen.getByRole("button", { name: WEBSITE }));
      await user.click(websiteButton());

      expect(props.onOpenWebsite).toHaveBeenCalledTimes(2);
      expect(props.onOpenWebsite).toHaveBeenNthCalledWith(1, WEBSITE);
      expect(props.onOpenWebsite).toHaveBeenNthCalledWith(2, WEBSITE);
    },
  );

  it("retains an API address link without creating a website shortcut", async () => {
    const user = userEvent.setup();
    const { props } = renderCard(
      provider({ websiteUrl: undefined, notes: "" }),
    );

    expect(
      screen.queryByRole("button", { name: /^访问官网/ }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: API_URL }));
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
    expect(props.onOpenWebsite).toHaveBeenCalledWith(API_URL);
  });

  it.each([
    undefined,
    "",
    "   ",
    "not a valid URL",
    "https://",
    "javascript:alert(1)",
    "file:///tmp/pricing.html",
    "mailto:help@example.com",
    "console.example.com/pricing",
    "//console.example.com/pricing",
  ])(
    "hides the shortcut for a missing or unsupported website: %j",
    (websiteUrl) => {
      const { props } = renderCard(provider({ websiteUrl }));

      expect(
        screen.queryByRole("button", { name: /^访问官网/ }),
      ).not.toBeInTheDocument();
      expect(screen.getByText(NOTE)).toBeInTheDocument();
      expect(props.onOpenWebsite).not.toHaveBeenCalled();
    },
  );

  it("refreshes the destination and removes the shortcut when the website is cleared", async () => {
    const user = userEvent.setup();
    const { props, rerenderProvider } = renderCard(provider());
    await user.click(websiteButton());

    const nextWebsite = "https://new.example.org/billing?currency=CNY#balance";
    rerenderProvider(provider({ websiteUrl: nextWebsite }));
    expect(
      screen.queryByRole("button", { name: "访问官网 · console.example.com" }),
    ).not.toBeInTheDocument();
    await user.click(websiteButton("new.example.org"));
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(2);
    expect(props.onOpenWebsite).toHaveBeenNthCalledWith(1, WEBSITE);
    expect(props.onOpenWebsite).toHaveBeenNthCalledWith(2, nextWebsite);

    rerenderProvider(provider({ websiteUrl: undefined }));
    expect(
      screen.queryByRole("button", { name: /^访问官网/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(NOTE)).toBeInTheDocument();
  });

  it("activates once per mouse, Enter, and Space gesture without editing, switching, or dragging", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    const onPointerDown = vi.fn();
    const onKeyDown = vi.fn();
    const { props } = renderCard(provider(), {
      presentation: {
        chips: [{ key: "custom", label: "Custom badge", tone: "outline" }],
        buttons: [
          { key: "switch", label: "Use this provider", onClick: onSwitch },
        ],
      },
      dragHandleProps: {
        attributes: {
          role: "button",
          tabIndex: 0,
          "aria-disabled": false,
          "aria-pressed": false,
          "aria-roledescription": "sortable",
          "aria-describedby": "drag-hint",
        },
        listeners: { onPointerDown, onKeyDown },
        isDragging: false,
      },
    });
    const button = websiteButton();
    const heading = screen.getByRole("heading", { name: "Relay provider" });
    const badge = screen.getByText("Custom badge");
    expect(heading.parentElement).toContainElement(button);
    expect(heading.parentElement).toContainElement(badge);
    expect(
      heading.compareDocumentPosition(button) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(
      button.compareDocumentPosition(badge) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    await user.click(button);
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
    button.focus();
    await user.keyboard("{Enter}");
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(2);
    await user.keyboard(" ");
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(3);
    expect(props.onEdit).not.toHaveBeenCalled();
    expect(props.onDelete).not.toHaveBeenCalled();
    expect(onSwitch).not.toHaveBeenCalled();
    expect(onPointerDown).not.toHaveBeenCalled();
    expect(onKeyDown).not.toHaveBeenCalled();

    // Keep the real action bar mounted: its own controls still work independently.
    await user.click(
      screen.getByRole("button", { name: "providerCard.action.edit" }),
    );
    await user.click(screen.getByRole("button", { name: "Use this provider" }));
    expect(props.onEdit).toHaveBeenCalledTimes(1);
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(3);
  });

  it.each([
    ["current", { isCurrent: true }],
    ["dimmed", { presentation: { chips: [], buttons: [], dim: true } }],
    [
      "editing unavailable",
      {
        presentation: {
          chips: [],
          buttons: [],
          editDisabledReason: "Managed elsewhere",
        },
      },
    ],
  ] satisfies [string, Partial<Omit<CardProps, "provider">>][])(
    "keeps the website available when the card is %s",
    async (_, options) => {
      const user = userEvent.setup();
      const { props } = renderCard(provider(), options);

      expect(websiteButton()).toBeEnabled();
      await user.click(websiteButton());
      expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
      expect(props.onOpenWebsite).toHaveBeenCalledWith(WEBSITE);
      expect(props.onEdit).not.toHaveBeenCalled();
    },
  );

  it("preserves the Codex native-login explanation alongside the website shortcut", async () => {
    const user = userEvent.setup();
    const { props } = renderCard(
      provider({
        id: "codex-official",
        name: "OpenAI Official",
        category: "official",
        notes: undefined,
        settingsConfig: { auth: {}, config: "" },
      }),
      { appId: "codex" },
    );

    expect(
      screen.getByText("账号会随 Codex CLI 当前登录变化"),
    ).toBeInTheDocument();
    await user.click(websiteButton());
    expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
    expect(props.onOpenWebsite).toHaveBeenCalledWith(WEBSITE);
  });

  it.each([undefined, "Primary managed account"])(
    "preserves the Codex managed account details with note %j",
    async (notes) => {
      const user = userEvent.setup();
      const login = "work@example.com";
      const { props } = renderCard(
        provider({
          name: "Work account",
          category: "official",
          notes,
          settingsConfig: { auth: {}, config: "" },
          meta: {
            authBinding: {
              source: "managed_account",
              authProvider: "codex_oauth",
              accountId: "work",
            },
          },
        }),
        {
          appId: "codex",
          authStatus: {
            provider: "codex_oauth",
            authenticated: true,
            default_account_id: "work",
            accounts: [
              {
                id: "work",
                provider: "codex_oauth",
                login,
                avatar_url: null,
                authenticated_at: 0,
                is_default: true,
                github_domain: "",
                reauth_required: true,
                requires_reauth: true,
              },
            ],
          },
        },
      );

      expect(screen.getByText(notes ?? login)).toBeInTheDocument();
      expect(screen.getByText("需要重新登录")).toBeInTheDocument();
      await user.click(websiteButton());
      expect(props.onOpenWebsite).toHaveBeenCalledTimes(1);
      expect(props.onOpenWebsite).toHaveBeenCalledWith(WEBSITE);
    },
  );
});
