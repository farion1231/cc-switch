import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { EnvironmentTargetsPanel } from "@/components/settings/EnvironmentTargetsPanel";

const apiMocks = vi.hoisted(() => ({
  listManagedTargets: vi.fn(),
  inspectManagedTarget: vi.fn(),
  migrateHistory: vi.fn(),
  restoreHistory: vi.fn(),
  getProviders: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    getAll: (...args: unknown[]) => apiMocks.getProviders(...args),
  },
  settingsApi: {
    listManagedTargets: (...args: unknown[]) =>
      apiMocks.listManagedTargets(...args),
    inspectManagedTarget: (...args: unknown[]) =>
      apiMocks.inspectManagedTarget(...args),
    migrateManagedTargetCodexHistory: (...args: unknown[]) =>
      apiMocks.migrateHistory(...args),
    restoreManagedTargetCodexHistory: (...args: unknown[]) =>
      apiMocks.restoreHistory(...args),
  },
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ takeoverStatus: { codex: false } }),
}));

vi.mock("@/lib/platform", () => ({ isWindows: () => true }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: toastMocks,
}));

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("EnvironmentTargetsPanel history migration", () => {
  beforeEach(() => {
    toastMocks.success.mockReset();
    toastMocks.error.mockReset();
    toastMocks.info.mockReset();
    apiMocks.listManagedTargets.mockReset().mockResolvedValue([
      {
        id: "wsl-ubuntu",
        app: "codex",
        name: "Ubuntu · tester",
        kind: { type: "wsl", distro: "Ubuntu", user: "tester" },
        configLocation: { path: "/home/tester/.codex" },
        currentProviderId: "pinai",
        managementState: "managed",
        providerOverrides: {},
      },
    ]);
    apiMocks.inspectManagedTarget.mockReset().mockResolvedValue({
      targetId: "wsl-ubuntu",
      reachable: true,
      config: "valid",
      auth: "valid",
      activeSessionCount: 48,
      archivedSessionCount: 0,
      stateDbPresent: true,
    });
    apiMocks.getProviders.mockReset().mockResolvedValue({});
    apiMocks.migrateHistory.mockReset().mockResolvedValue({
      changedJsonlFiles: 48,
      changedStateRows: 48,
      backupPath: "/home/tester/.cc-switch/backups/generation",
    });
    apiMocks.restoreHistory.mockReset().mockResolvedValue({
      changedJsonlFiles: 48,
      changedStateRows: 48,
      backupPath: "/home/tester/.cc-switch/backups/restore",
    });
  });

  it("requires confirmation and migrates only the selected Target", async () => {
    render(<EnvironmentTargetsPanel />, { wrapper });

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.environments.migrateHistory",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.environments.confirmMigration",
      }),
    );

    await waitFor(() => {
      expect(apiMocks.migrateHistory).toHaveBeenCalledWith("wsl-ubuntu");
    });
    expect(apiMocks.restoreHistory).not.toHaveBeenCalled();
  });

  it("closes the confirmation immediately while WSL migration continues", async () => {
    let resolveMigration!: (value: {
      changedJsonlFiles: number;
      changedStateRows: number;
      backupPath: string;
    }) => void;
    apiMocks.migrateHistory.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveMigration = resolve;
        }),
    );
    render(<EnvironmentTargetsPanel />, { wrapper });

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.environments.migrateHistory",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.environments.confirmMigration",
      }),
    );

    expect(
      screen.queryByRole("button", {
        name: "settings.environments.confirmMigration",
      }),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(apiMocks.migrateHistory).toHaveBeenCalledTimes(1);
    });

    resolveMigration({
      changedJsonlFiles: 48,
      changedStateRows: 48,
      backupPath: "/home/tester/.cc-switch/backups/generation",
    });
  });

  it("hides history actions until the Target is managed", async () => {
    apiMocks.listManagedTargets.mockResolvedValue([
      {
        id: "wsl-ubuntu",
        app: "codex",
        name: "Ubuntu · tester",
        kind: { type: "wsl", distro: "Ubuntu", user: "tester" },
        configLocation: { path: "/home/tester/.codex" },
        currentProviderId: "pinai",
        managementState: "unmanaged",
        providerOverrides: {},
      },
    ]);
    render(<EnvironmentTargetsPanel />, { wrapper });

    await screen.findByText("Ubuntu · tester");
    expect(
      screen.queryByRole("button", {
        name: "settings.environments.migrateHistory",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "settings.environments.restoreHistory",
      }),
    ).not.toBeInTheDocument();
  });

  it("maps restore skip reasons to distinct toast keys", async () => {
    toastMocks.info.mockReset();
    apiMocks.restoreHistory.mockResolvedValue({
      changedJsonlFiles: 0,
      changedStateRows: 0,
      skippedReason: "no_backup_ledger",
    });
    render(<EnvironmentTargetsPanel />, { wrapper });

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.environments.restoreHistory",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.environments.confirmRestore",
      }),
    );

    await waitFor(() => {
      expect(toastMocks.info).toHaveBeenCalledWith(
        "settings.environments.historyNoBackupLedger",
      );
    });
    expect(toastMocks.success).not.toHaveBeenCalled();
  });

  it("maps nothing_to_restore separately from no_backup_ledger", async () => {
    toastMocks.info.mockReset();
    apiMocks.restoreHistory.mockResolvedValue({
      changedJsonlFiles: 0,
      changedStateRows: 0,
      skippedReason: "nothing_to_restore",
    });
    render(<EnvironmentTargetsPanel />, { wrapper });

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.environments.restoreHistory",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.environments.confirmRestore",
      }),
    );

    await waitFor(() => {
      expect(toastMocks.info).toHaveBeenCalledWith(
        "settings.environments.historyNothingToRestore",
      );
    });
  });
});
