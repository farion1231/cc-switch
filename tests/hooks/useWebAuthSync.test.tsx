import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";

const clearAuthTokenMock = vi.fn();

let isTauriMock = false;

vi.mock("@/lib/api/web-client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/web-client")>();
  return {
    ...actual,
    clearAuthToken: () => clearAuthTokenMock(),
  };
});

vi.mock("@/lib/environment", () => ({
  isTauri: () => isTauriMock,
  isLinux: () => false,
  isMac: () => false,
  isWindows: () => false,
}));

import { useWebAuthSync } from "@/hooks/useWebAuthSync";

function TestHarness({ initialAuth = false }: { initialAuth?: boolean }) {
  const [auth, setAuth] = useState(initialAuth);
  useWebAuthSync(auth, setAuth);
  return <div data-testid="auth-state">{auth ? "in" : "out"}</div>;
}

function renderHarness(initialAuth = false) {
  const client = new QueryClient();
  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <TestHarness initialAuth={initialAuth} />
      </QueryClientProvider>,
    ),
  };
}

describe("useWebAuthSync", () => {
  beforeEach(() => {
    isTauriMock = false;
    window.history.replaceState({}, "", "/");
    clearAuthTokenMock.mockClear();
  });

  afterEach(() => {
    isTauriMock = false;
    window.history.replaceState({}, "", "/");
  });

  it("syncs URL to /login when unauthenticated", () => {
    renderHarness(false);
    expect(window.location.pathname).toBe("/login");
    expect(screen.getByTestId("auth-state")).toHaveTextContent("out");
  });

  it("syncs URL to / when authenticated", () => {
    window.history.replaceState({}, "", "/login");
    renderHarness(true);
    expect(window.location.pathname).toBe("/");
    expect(screen.getByTestId("auth-state")).toHaveTextContent("in");
  });

  it("handles auth:expired by clearing auth, cache, and URL", async () => {
    const { client } = renderHarness(true);
    client.setQueryData(["test"], "value");

    act(() => {
      window.dispatchEvent(new CustomEvent("auth:expired"));
    });

    await waitFor(() =>
      expect(screen.getByTestId("auth-state")).toHaveTextContent("out"),
    );
    expect(window.location.pathname).toBe("/login");
    expect(clearAuthTokenMock).toHaveBeenCalledTimes(1);
    expect(client.getQueryData(["test"])).toBeUndefined();
  });

  it("does not call clearAuthToken after unmount", () => {
    const { unmount } = renderHarness(true);
    unmount();

    act(() => {
      window.dispatchEvent(new CustomEvent("auth:expired"));
    });

    expect(clearAuthTokenMock).not.toHaveBeenCalled();
  });

  it("no-ops in Tauri mode", () => {
    isTauriMock = true;
    window.history.replaceState({}, "", "/login");
    renderHarness(true);

    expect(window.location.pathname).toBe("/login");

    act(() => {
      window.dispatchEvent(new CustomEvent("auth:expired"));
    });

    expect(clearAuthTokenMock).not.toHaveBeenCalled();
  });
});
