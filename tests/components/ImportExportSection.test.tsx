import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ImportExportSection } from "@/components/settings/ImportExportSection";

const tMock = vi.fn((key: string) => key);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

describe("ImportExportSection Component", () => {
  const baseProps = {
    status: "idle" as const,
    selectedFile: "",
    errorMessage: null,
    backupId: null,
    isImporting: false,
    onImport: vi.fn(),
    onExport: vi.fn(),
    onClear: vi.fn(),
  };

  beforeEach(() => {
    tMock.mockImplementation((key: string) => key);
    baseProps.onImport.mockReset();
    baseProps.onExport.mockReset();
    baseProps.onClear.mockReset();
  });

  it("should start import from the primary button when no file is selected", () => {
    render(<ImportExportSection {...baseProps} />);

    const importButton = screen.getByRole("button", {
      name: "settings.import",
    });
    expect(importButton).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.exportConfig" }),
    );
    expect(baseProps.onExport).toHaveBeenCalledTimes(1);

    fireEvent.click(importButton);
    expect(baseProps.onImport).toHaveBeenCalledTimes(1);
  });

  it("should show filename and enable import/clear when file is selected", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
      />,
    );

    expect(screen.getByText(/config\.json/)).toBeInTheDocument();
    const importButton = screen.getByRole("button", {
      name: /settings\.import/,
    });
    expect(importButton).toBeEnabled();
    fireEvent.click(importButton);
    expect(baseProps.onImport).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    expect(baseProps.onClear).toHaveBeenCalledTimes(1);
  });

  it("should show loading text and disable import button during import", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
        isImporting
        status="importing"
      />,
    );

    const importingLabels = screen.getAllByText("settings.importing");
    expect(importingLabels.length).toBeGreaterThanOrEqual(2);
    expect(
      screen.getByRole("button", { name: "settings.importing" }),
    ).toBeDisabled();
    expect(screen.getByText("common.loading")).toBeInTheDocument();
  });

  it("should display backup information on successful import", () => {
    render(
      <ImportExportSection
        {...baseProps}
        selectedFile={"/tmp/test/config.json"}
        status="success"
        backupId="backup-001"
      />,
    );

    expect(screen.getByText("settings.importSuccess")).toBeInTheDocument();
    expect(screen.getByText(/backup-001/)).toBeInTheDocument();
    expect(screen.getByText("settings.autoReload")).toBeInTheDocument();
  });

  it("should display error message when import fails", () => {
    render(
      <ImportExportSection
        {...baseProps}
        status="error"
        errorMessage="Parse failed"
      />,
    );

    expect(screen.getByText("settings.importFailed")).toBeInTheDocument();
    expect(screen.getByText("Parse failed")).toBeInTheDocument();
  });
});
