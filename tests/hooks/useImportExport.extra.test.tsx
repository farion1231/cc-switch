import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useImportExport } from "@/hooks/useImportExport";

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

const openFileDialogMock = vi.fn();
const importConfigMock = vi.fn();
const saveFileDialogMock = vi.fn();
const exportConfigMock = vi.fn();
const syncCurrentProvidersLiveMock = vi.fn();

vi.mock("@/lib/api", () => ({
  settingsApi: {
    openFileDialog: (...args: unknown[]) => openFileDialogMock(...args),
    importConfigFromFile: (...args: unknown[]) => importConfigMock(...args),
    saveFileDialog: (...args: unknown[]) => saveFileDialogMock(...args),
    exportConfigToFile: (...args: unknown[]) => exportConfigMock(...args),
    syncCurrentProvidersLive: (...args: unknown[]) =>
      syncCurrentProvidersLiveMock(...args),
  },
}));

describe("useImportExport Hook (edge cases)", () => {
  beforeEach(() => {
    openFileDialogMock.mockReset();
    importConfigMock.mockReset();
    saveFileDialogMock.mockReset();
    exportConfigMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
    syncCurrentProvidersLiveMock.mockReset();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps state unchanged when file dialog resolves to null", async () => {
    openFileDialogMock.mockResolvedValue(null);
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });

    expect(result.current.selectedFile).toBe("");
    expect(result.current.status).toBe("idle");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("resetStatus clears errors but preserves selected file", async () => {
    openFileDialogMock.mockResolvedValue("/config.json");
    importConfigMock.mockResolvedValue({ success: false, message: "broken" });
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });

    await act(async () => {
      await result.current.importConfig();
    });

    act(() => {
      result.current.resetStatus();
    });

    expect(result.current.selectedFile).toBe("/config.json");
    expect(result.current.status).toBe("idle");
    expect(result.current.errorMessage).toBeNull();
    expect(result.current.backupId).toBeNull();
  });

  it("does not call onImportSuccess when import fails", async () => {
    openFileDialogMock.mockResolvedValue("/config.json");
    importConfigMock.mockResolvedValue({
      success: false,
      message: "invalid",
    });
    const onImportSuccess = vi.fn();
    const { result } = renderHook(() => useImportExport({ onImportSuccess }));

    await act(async () => {
      await result.current.selectImportFile();
    });

    await act(async () => {
      await result.current.importConfig();
    });

    expect(onImportSuccess).not.toHaveBeenCalled();
    expect(result.current.status).toBe("error");
  });

  it("propagates export success message to toast with saved path", async () => {
    saveFileDialogMock.mockResolvedValue("/exports/config.json");
    exportConfigMock.mockResolvedValue({
      success: true,
      filePath: "/final/config.json",
    });
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.exportConfig();
    });

    expect(exportConfigMock).toHaveBeenCalledWith("/exports/config.json");
    expect(toastSuccessMock).toHaveBeenCalledWith(
      expect.stringContaining("/final/config.json"),
      expect.objectContaining({ closeButton: true }),
    );
  });
});
