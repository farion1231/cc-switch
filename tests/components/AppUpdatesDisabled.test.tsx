import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { check } from "@tauri-apps/plugin-updater";
import { DatabaseUpgrade } from "@/components/DatabaseUpgrade";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));

afterEach(() => vi.useRealTimers());

describe("custom build with application updates disabled", () => {
  it("reports disabled without contacting the updater", async () => {
    await expect(checkForUpdate()).resolves.toEqual({ status: "disabled" });
    expect(check).not.toHaveBeenCalled();
  });

  it("never checks on startup or when the update context is called", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useUpdate(), {
      wrapper: UpdateProvider,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
      expect(await result.current.checkUpdate()).toBe(false);
    });
    expect(check).not.toHaveBeenCalled();
    expect(result.current.hasUpdate).toBe(false);
    expect(result.current.isChecking).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("keeps database recovery available without checking or offering an upstream upgrade", () => {
    render(
      <DatabaseUpgrade payload={{ db_version: 21, supported_version: 20 }} />,
    );

    expect(invoke).not.toHaveBeenCalled();
    expect(
      screen.getByText("dbUpgrade.updatesDisabledDescription"),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "升级应用" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("升级也无法解决")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "打开配置目录" }));
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("open_app_config_folder");
    expect(screen.getByRole("button", { name: "退出" })).toBeEnabled();
  });
});
