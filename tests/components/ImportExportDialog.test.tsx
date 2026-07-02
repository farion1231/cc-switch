import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ImportExportDialog } from "@/components/providers/ImportExportDialog";
import type { Provider } from "@/types";

// Mock i18next
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: any) => {
      const translations: Record<string, string> = {
        "provider.importExport.title": "导入/导出供应商配置",
        "provider.importExport.description": "导出供应商配置为 JSON 文件，或从文件导入配置",
        "provider.importExport.export": "导出",
        "provider.importExport.import": "导入",
        "provider.importExport.selectAll": "全选",
        "provider.importExport.noProviders": "没有可导出的供应商",
        "provider.importExport.selectedCount": `已选择 ${options?.count || 0} 个供应商`,
        "provider.importExport.selectFile": "选择文件",
        "provider.importExport.chooseFile": "选择 JSON 文件",
        "provider.importExport.jsonContent": "或粘贴 JSON 内容",
        "provider.importExport.jsonPlaceholder": "粘贴导出的 JSON 配置...",
        "provider.importExport.exportButton": `导出 ${options?.count || 0} 个`,
        "provider.importExport.importButton": "导入",
        "provider.importExport.exporting": "导出中...",
        "provider.importExport.importing": "导入中...",
        "common.cancel": "取消",
      };
      return translations[key] || key;
    },
  }),
}));

describe("ImportExportDialog", () => {
  const mockProviders: Record<string, Provider> = {
    "provider-1": {
      id: "provider-1",
      name: "Provider 1",
      settingsConfig: {},
      category: "custom",
      createdAt: Date.now(),
      inFailoverQueue: false,
    },
    "provider-2": {
      id: "provider-2",
      name: "Provider 2",
      settingsConfig: {},
      category: "custom",
      createdAt: Date.now(),
      inFailoverQueue: false,
    },
  };

  const mockOnClose = vi.fn();
  const mockOnExport = vi.fn();
  const mockOnImport = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders dialog when open", () => {
    render(
      <ImportExportDialog
        isOpen={true}
        onClose={mockOnClose}
        appId="claude"
        providers={mockProviders}
        onExport={mockOnExport}
        onImport={mockOnImport}
      />,
    );

    expect(screen.getByText("导入/导出供应商配置")).toBeInTheDocument();
  });

  it("does not render when closed", () => {
    const { container } = render(
      <ImportExportDialog
        isOpen={false}
        onClose={mockOnClose}
        appId="claude"
        providers={mockProviders}
        onExport={mockOnExport}
        onImport={mockOnImport}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  describe("Export Tab", () => {
    it("shows list of providers for export", () => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      expect(screen.getByText("Provider 1")).toBeInTheDocument();
      expect(screen.getByText("Provider 2")).toBeInTheDocument();
    });

    it("shows empty message when no providers", () => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={{}}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      expect(screen.getByText("没有可导出的供应商")).toBeInTheDocument();
    });

    it("allows selecting providers", () => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      const checkbox = screen.getByRole("checkbox", { name: /Provider 1/i });

      expect(checkbox).not.toBeChecked();
      fireEvent.click(checkbox);
      expect(checkbox).toBeChecked();
    });

    it("handles select all for export", () => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      const selectAllCheckbox = screen.getByRole("checkbox", { name: /全选/i });
      const provider1Checkbox = screen.getByRole("checkbox", { name: /Provider 1/i });
      const provider2Checkbox = screen.getByRole("checkbox", { name: /Provider 2/i });

      fireEvent.click(selectAllCheckbox);
      expect(provider1Checkbox).toBeChecked();
      expect(provider2Checkbox).toBeChecked();
    });

    it("disables export button when no providers selected", () => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      const exportButton = screen.getByRole("button", { name: /导出 0 个/i });
      expect(exportButton).toBeDisabled();
    });

    it("calls onExport with selected provider IDs", async () => {
      mockOnExport.mockResolvedValue(undefined);

      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      const provider1Checkbox = screen.getByRole("checkbox", { name: /Provider 1/i });
      fireEvent.click(provider1Checkbox);

      const exportButton = screen.getByRole("button", { name: /导出 1 个/i });
      fireEvent.click(exportButton);

      await waitFor(() => {
        expect(mockOnExport).toHaveBeenCalledWith(["provider-1"]);
      });
    });
  });

  describe("Import Tab", () => {
    beforeEach(() => {
      render(
        <ImportExportDialog
          isOpen={true}
          onClose={mockOnClose}
          appId="claude"
          providers={mockProviders}
          onExport={mockOnExport}
          onImport={mockOnImport}
        />,
      );

      // Switch to import tab
      const importTab = screen.getByRole("tab", { name: /导入/i });
      fireEvent.click(importTab);
    });

    it("shows import interface", () => {
      expect(screen.getByText("选择文件")).toBeInTheDocument();
      expect(screen.getByPlaceholderText("粘贴导出的 JSON 配置...")).toBeInTheDocument();
    });

    it("allows pasting JSON content", () => {
      const textarea = screen.getByPlaceholderText("粘贴导出的 JSON 配置...");
      const jsonContent = JSON.stringify({ test: "data" });

      fireEvent.change(textarea, { target: { value: jsonContent } });
      expect(textarea).toHaveValue(jsonContent);
    });

    it("disables import button when no JSON content", () => {
      const importButton = screen.getByRole("button", { name: /导入/i });
      expect(importButton).toBeDisabled();
    });

    it("enables import button when JSON content is present", () => {
      const textarea = screen.getByPlaceholderText("粘贴导出的 JSON 配置...");
      fireEvent.change(textarea, { target: { value: '{"test": "data"}' } });

      const importButton = screen.getByRole("button", { name: /导入/i });
      expect(importButton).not.toBeDisabled();
    });

    it("calls onImport with JSON content", async () => {
      mockOnImport.mockResolvedValue(undefined);

      const textarea = screen.getByPlaceholderText("粘贴导出的 JSON 配置...");
      const jsonContent = '{"test": "data"}';
      fireEvent.change(textarea, { target: { value: jsonContent } });

      const importButton = screen.getByRole("button", { name: /导入/i });
      fireEvent.click(importButton);

      await waitFor(() => {
        expect(mockOnImport).toHaveBeenCalledWith(jsonContent);
      });
    });

    it("clears textarea after successful import", async () => {
      mockOnImport.mockResolvedValue(undefined);

      const textarea = screen.getByPlaceholderText("粘贴导出的 JSON 配置...");
      fireEvent.change(textarea, { target: { value: '{"test": "data"}' } });

      const importButton = screen.getByRole("button", { name: /导入/i });
      fireEvent.click(importButton);

      await waitFor(() => {
        expect(textarea).toHaveValue("");
      });
    });
  });

  it("calls onClose when cancel button clicked", () => {
    render(
      <ImportExportDialog
        isOpen={true}
        onClose={mockOnClose}
        appId="claude"
        providers={mockProviders}
        onExport={mockOnExport}
        onImport={mockOnImport}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: /取消/i });
    fireEvent.click(cancelButton);

    expect(mockOnClose).toHaveBeenCalled();
  });

  it("prevents closing during export operation", async () => {
    mockOnExport.mockImplementation(() => new Promise(() => {})); // Never resolves

    render(
      <ImportExportDialog
        isOpen={true}
        onClose={mockOnClose}
        appId="claude"
        providers={mockProviders}
        onExport={mockOnExport}
        onImport={mockOnImport}
      />,
    );

    const provider1Checkbox = screen.getByRole("checkbox", { name: /Provider 1/i });
    fireEvent.click(provider1Checkbox);

    const exportButton = screen.getByRole("button", { name: /导出 1 个/i });
    fireEvent.click(exportButton);

    // Try to close dialog
    const cancelButton = screen.getByRole("button", { name: /取消/i });
    fireEvent.click(cancelButton);

    // Should not close during export
    expect(mockOnClose).not.toHaveBeenCalled();
  });
});
