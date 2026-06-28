import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  resetProviderState,
  setProviders,
  setCurrentProviderId,
} from "../msw/state";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("@/lib/environment", () => ({
  isTauri: () => true,
  isWebMode: () => true,
  isDesktop: () => true,
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open }: any) => (open ? <div data-testid="add-provider-dialog">add</div> : null),
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({ open }: any) => (open ? <div data-testid="edit-provider-dialog">edit</div> : null),
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen }: any) => (isOpen ? <div data-testid="usage-modal">usage</div> : null),
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => <button onClick={onClick}>update-badge</button>,
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
};

describe("App logout", () => {
  beforeEach(() => {
    resetProviderState();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
  });

  it("renders logout button in web mode", async () => {
    setProviders("claude", {
      "claude-1": {
        id: "claude-1",
        name: "Claude Provider",
        settingsConfig: {},
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("claude", "claude-1");

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    expect(screen.getByTitle("common.logout")).toBeInTheDocument();
  });

  it("opens logout confirmation when logout button is clicked", async () => {
    setProviders("claude", {
      "claude-1": {
        id: "claude-1",
        name: "Claude Provider",
        settingsConfig: {},
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("claude", "claude-1");

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "claude-1",
      ),
    );

    fireEvent.click(screen.getByTitle("common.logout"));

    await waitFor(() =>
      expect(document.body).toHaveAttribute("data-scroll-locked"),
    );
  });
});
