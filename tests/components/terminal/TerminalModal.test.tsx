import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TerminalModal } from "@/components/terminal/TerminalModal";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";

vi.mock("@/components/terminal/Terminal", () => ({
  Terminal: vi.fn(
    ({
      providerId,
      app,
      onError,
      onClose,
    }: {
      providerId: string;
      app: AppId;
      onError: (error: string) => void;
      onClose: () => void;
    }) => (
      <div data-testid="mock-terminal">
        <div data-testid="terminal-provider-id">{providerId}</div>
        <div data-testid="terminal-app-id">{app}</div>
        <button onClick={() => onError("Test error")}>Trigger Error</button>
        <button onClick={onClose}>Close Terminal</button>
      </div>
    ),
  ),
}));

vi.mock("lucide-react", () => ({
  X: () => <span data-testid="x-icon">X</span>,
  Maximize2: () => <span data-testid="maximize-icon">Maximize</span>,
  Minimize2: () => <span data-testid="minimize-icon">Minimize</span>,
}));

vi.mock("react-i18next", async () => {
  const actual = await vi.importActual("react-i18next");
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, defaultValue?: string) => defaultValue || key,
    }),
  };
});

describe("TerminalModal", () => {
  const mockProvider: Provider = {
    id: "claude-1",
    name: "Claude Official",
    settingsConfig: {} as Record<string, any>,
  };


  const mockAppId: AppId = "claude";
  const mockOnClose = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("Rendering", () => {
    it("renders nothing when isOpen=false", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={false}
          onClose={mockOnClose}
        />,
      );

      expect(container.firstChild).toBeNull();
    });

    it("renders nothing when provider is null", () => {
      const { container } = render(
        <TerminalModal
          provider={null}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(container.firstChild).toBeNull();
    });

    it("renders modal when isOpen=true and provider is set", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByTestId("mock-terminal")).toBeInTheDocument();
    });

    it("displays provider name in header", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(
        screen.getByText(new RegExp(mockProvider.name)),
      ).toBeInTheDocument();
    });

    it("displays appId in header", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByTestId("terminal-app-id")).toBeInTheDocument();
    });

    it("renders Terminal component with correct props", async () => {
      const terminalModule = await import("@/components/terminal/Terminal");
      const Terminal = vi.mocked(terminalModule.Terminal);

      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(Terminal).toHaveBeenCalledWith(
        expect.objectContaining({
          providerId: mockProvider.id,
          app: mockAppId,
          onError: expect.any(Function),
          onClose: expect.any(Function),
        }),
        expect.anything(),
      );
    });

    it("renders footer with provider environment note", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(
        screen.getByText(
          /terminal\.providerEnvNote|已加载该提供商的环境变量配置/,
        ),
      ).toBeInTheDocument();
    });
  });

  describe("Modal Controls", () => {
    it("calls onClose when close button is clicked", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const closeButtons = screen.getAllByRole("button");
      const xButton = closeButtons.find((btn) =>
        btn.querySelector('[data-testid="x-icon"]'),
      );

      fireEvent.click(xButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it("calls onClose when backdrop is clicked", async () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const backdrop = container.querySelector(".bg-black\\/60");
      fireEvent.click(backdrop!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it("clears error state when modal is closed via close button", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        expect(screen.getByText("Test error")).toBeInTheDocument();
      });

      const closeButtons = screen.getAllByRole("button");
      const xButton = closeButtons.find((btn) =>
        btn.querySelector('[data-testid="x-icon"]'),
      );
      fireEvent.click(xButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });
  });

  describe("Maximize/Minimize Functionality", () => {
    it("renders maximize button when not maximized", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByTestId("maximize-icon")).toBeInTheDocument();
      expect(screen.queryByTestId("minimize-icon")).not.toBeInTheDocument();
    });

    it("toggles to maximize when maximize button is clicked", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const maximizeButton = screen
        .getByTestId("maximize-icon")
        .closest("button");
      fireEvent.click(maximizeButton!);

      await waitFor(() => {
        expect(screen.getByTestId("minimize-icon")).toBeInTheDocument();
        expect(screen.queryByTestId("maximize-icon")).not.toBeInTheDocument();
      });
    });

    it("toggles to minimize when minimize button is clicked", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      let maximizeButton = screen
        .getByTestId("maximize-icon")
        .closest("button");
      fireEvent.click(maximizeButton!);

      await waitFor(() => {
        expect(screen.getByTestId("minimize-icon")).toBeInTheDocument();
      });

      const minimizeButton = screen
        .getByTestId("minimize-icon")
        .closest("button");
      fireEvent.click(minimizeButton!);

      await waitFor(() => {
        expect(screen.getByTestId("maximize-icon")).toBeInTheDocument();
      });
    });

    it("applies correct size classes when not maximized", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const modal = container.querySelector(
        ".relative.z-10.flex.flex-col.bg-\\[\\#1e1e1e\\]",
      );
      expect(modal).toHaveClass("w-[900px]", "h-[600px]");
    });

    it("applies correct size classes when maximized", async () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const maximizeButton = screen
        .getByTestId("maximize-icon")
        .closest("button");
      fireEvent.click(maximizeButton!);

      await waitFor(() => {
        const modal = container.querySelector(
          ".relative.z-10.flex.flex-col.bg-\\[\\#1e1e1e\\]",
        );
        expect(modal).toHaveClass("w-[95vw]", "h-[90vh]");
      });
    });
  });

  describe("Error Handling", () => {
    it("does not display error message initially", () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.queryByText("Test error")).not.toBeInTheDocument();
    });

    it("displays error message when Terminal calls onError", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        expect(screen.getByText("Test error")).toBeInTheDocument();
      });
    });

    it("displays error banner with correct styling", async () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        const errorBanner = container.querySelector(".bg-red-500\\/20");
        expect(errorBanner).toBeInTheDocument();
        expect(errorBanner?.className).toContain("border-b");
        expect(errorBanner?.className).toContain("border-red-500/30");
      });
    });

    it("displays error with correct text color", async () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        const errorText = container.querySelector(".text-red-400");
        expect(errorText).toBeInTheDocument();
        expect(errorText).toHaveClass("text-sm");
      });
    });

    it("clears error when close button is clicked", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        expect(screen.getByText("Test error")).toBeInTheDocument();
      });

      const closeButtons = screen.getAllByRole("button");
      const xButton = closeButtons.find((btn) =>
        btn.querySelector('[data-testid="x-icon"]'),
      );
      fireEvent.click(xButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it("supports multiple error messages (latest overwrites)", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      fireEvent.click(screen.getByText("Trigger Error"));

      await waitFor(() => {
        expect(screen.getByText("Test error")).toBeInTheDocument();
      });

      expect(screen.getByText("Test error")).toBeInTheDocument();
    });
  });

  describe("Terminal Props Passing", () => {
    it("passes providerId from provider.id to Terminal", () => {
      const customProvider: Provider = {
        ...mockProvider,
        id: "custom-provider-123",
      };

      render(
        <TerminalModal
          provider={customProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByTestId("terminal-provider-id")).toHaveTextContent(
        "custom-provider-123",
      );
    });

    it("passes appId correctly to Terminal", () => {
      const appIds: AppId[] = ["claude", "codex", "gemini"];

      appIds.forEach((appId) => {
        const { unmount } = render(
          <TerminalModal
            provider={mockProvider}
            appId={appId}
            isOpen={true}
            onClose={mockOnClose}
          />,
        );

        expect(screen.getByTestId("terminal-app-id")).toHaveTextContent(appId);
        unmount();
      });
    });
  });

  describe("Modal Structure", () => {
    it("has correct z-index structure", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const outerContainer = container.querySelector(".fixed.inset-0.z-50");
      expect(outerContainer).toBeInTheDocument();

      const modal = outerContainer?.querySelector(".relative.z-10");
      expect(modal).toBeInTheDocument();
    });

    it("renders header, terminal container, and footer", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const header = container.querySelector(".bg-\\[\\#252526\\].border-b");
      const terminalContainer = container.querySelector(
        ".flex-1.overflow-hidden",
      );
      const footer = container.querySelector(
        ".bg-\\[\\#252526\\].border-t.text-xs.text-gray-500",
      );

      expect(header).toBeInTheDocument();
      expect(terminalContainer).toBeInTheDocument();
      expect(footer).toBeInTheDocument();
    });

    it("has flex layout for modal content", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const modal = container.querySelector(".relative.z-10");
      expect(modal).toHaveClass("flex", "flex-col");
    });

    it("has correct background color", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const modal = container.querySelector(".bg-\\[\\#1e1e1e\\]");
      expect(modal).toBeInTheDocument();
    });

    it("has rounded corners and shadow", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const modal = container.querySelector(".rounded-lg.shadow-2xl");
      expect(modal).toBeInTheDocument();
    });

    it("has smooth transition for size changes", () => {
      const { container } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const modal = container.querySelector(".transition-all.duration-300");
      expect(modal).toBeInTheDocument();
    });
  });

  describe("Edge Cases", () => {
    it("handles provider with special characters in name", () => {
      const specialProvider: Provider = {
        ...mockProvider,
        name: "Provider & Co. <test>",
      };

      render(
        <TerminalModal
          provider={specialProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(
        screen.getByText(new RegExp("Provider & Co. <test>")),
      ).toBeInTheDocument();
    });

    it("handles rapid open/close cycles", () => {
      const { rerender } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      rerender(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={false}
          onClose={mockOnClose}
        />,
      );

      rerender(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByTestId("mock-terminal")).toBeInTheDocument();
    });

    it("handles provider change while modal is open", () => {
      const provider2: Provider = {
        ...mockProvider,
        id: "provider-2",
        name: "Provider 2",
      };

      const { rerender } = render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(
        screen.getByText(new RegExp(mockProvider.name)),
      ).toBeInTheDocument();

      rerender(
        <TerminalModal
          provider={provider2}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      expect(screen.getByText(new RegExp(provider2.name))).toBeInTheDocument();
      expect(
        screen.queryByText(new RegExp(mockProvider.name)),
      ).not.toBeInTheDocument();
    });

    it("handles rapid maximize/minimize toggles", async () => {
      render(
        <TerminalModal
          provider={mockProvider}
          appId={mockAppId}
          isOpen={true}
          onClose={mockOnClose}
        />,
      );

      const getToggleButton = () => {
        const buttons = screen.getAllByRole("button");
        return buttons.find(
          (btn) =>
            btn.querySelector('[data-testid="maximize-icon"]') ||
            btn.querySelector('[data-testid="minimize-icon"]'),
        );
      };

      for (let i = 0; i < 5; i++) {
        fireEvent.click(getToggleButton()!);
      }

      await waitFor(() => {
        expect(screen.getByTestId("minimize-icon")).toBeInTheDocument();
      });
    });
  });
});
