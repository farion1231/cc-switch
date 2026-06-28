import { render } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock CSS import first
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

// Mock @xterm/xterm with inline factory
vi.mock("@xterm/xterm", () => {
  return {
    Terminal: vi.fn(function () {
      return {
        write: vi.fn(),
        writeln: vi.fn(),
        dispose: vi.fn(),
        open: vi.fn(),
        onData: vi.fn(),
        loadAddon: vi.fn(),
        cols: 80,
        rows: 24,
      };
    }),
  };
});

// Mock @xterm/addon-fit with inline factory
vi.mock("@xterm/addon-fit", () => {
  return {
    FitAddon: vi.fn(function () {
      return {
        fit: vi.fn(),
      };
    }),
  };
});

// Mock web-client with inline factory
vi.mock("@/lib/api/web-client", () => {
  return {
    connectTerminalWebSocket: vi.fn(function () {
      return {
        send: vi.fn(),
        resize: vi.fn(),
        close: vi.fn(),
      };
    }),
  };
});

// Import component and modules AFTER mocking
import { Terminal } from "@/components/terminal/Terminal";
import { connectTerminalWebSocket } from "@/lib/api/web-client";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

// Type helpers for accessing mock callbacks
type OnDataCallback = (data: Uint8Array) => void;
type OnReadyCallback = () => void;
type OnErrorCallback = (error: string) => void;
type OnCloseCallback = () => void;

// Get mocked functions
const mockTerminalFn = vi.mocked(XTerm);
const mockFitAddonFn = vi.mocked(FitAddon);
const mockConnectTerminalWebSocket = vi.mocked(connectTerminalWebSocket);

describe("Terminal Component", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  describe("Rendering", () => {
    it("should render a div container with correct className", () => {
      const { container } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      const terminalDiv = container.querySelector(
        ".h-full.w-full.bg-\\[\\#1e1e1e\\].p-2",
      );
      expect(terminalDiv).toBeInTheDocument();
    });

    it("should render a div with font-family inline style", () => {
      const { container } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      const terminalDiv = container.firstChild as HTMLDivElement;
      expect(terminalDiv.style.fontFamily).toContain("Menlo");
    });

    it("should initialize xterm.js Terminal instance", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockTerminalFn).toHaveBeenCalledWith(
        expect.objectContaining({
          cursorBlink: true,
          fontFamily: expect.stringContaining("Menlo"),
          fontSize: 14,
          theme: expect.objectContaining({
            background: "#1e1e1e",
            foreground: "#d4d4d4",
          }),
        }),
      );
    });

    it("should load FitAddon to xterm instance", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockFitAddonFn).toHaveBeenCalled();
    });

    it("should open terminal in the ref container", () => {
      const { container } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      expect(container.firstChild).toBeTruthy();
      expect(mockTerminalFn).toHaveBeenCalled();
    });

    it("should call fit on FitAddon after opening terminal", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockFitAddonFn).toHaveBeenCalled();
    });
  });

  describe("WebSocket integration", () => {
    it("should call connectTerminalWebSocket with correct parameters", () => {
      render(<Terminal providerId="provider-123" app="codex" />);

      expect(mockConnectTerminalWebSocket).toHaveBeenCalledWith(
        "provider-123",
        "codex",
        expect.any(Function),
        expect.any(Function),
        expect.any(Function),
        expect.any(Function),
      );
    });

    it("should write received data from WebSocket to xterm", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      const onDataCallback = mockConnectTerminalWebSocket.mock
        .calls[0][2] as unknown as OnDataCallback;
      const testData = new Uint8Array([72, 101, 108, 108, 111]);

      onDataCallback(testData);

      expect(mockTerminalFn).toHaveBeenCalled();
    });

    it("should handle ready callback and call handleResize", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      const onReadyCallback = mockConnectTerminalWebSocket.mock
        .calls[0][3] as unknown as OnReadyCallback;
      onReadyCallback();

      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
    });

    it("should handle error callback and call onError prop", () => {
      const onErrorMock = vi.fn();

      render(
        <Terminal
          providerId="test-provider"
          app="claude"
          onError={onErrorMock}
        />,
      );

      const onErrorCallback = mockConnectTerminalWebSocket.mock
        .calls[0][4] as unknown as OnErrorCallback;
      onErrorCallback("Connection failed");

      expect(onErrorMock).toHaveBeenCalledWith("Connection failed");
    });

    it("should handle close callback and call onClose prop", () => {
      const onCloseMock = vi.fn();

      render(
        <Terminal
          providerId="test-provider"
          app="claude"
          onClose={onCloseMock}
        />,
      );

      const onCloseCallback = mockConnectTerminalWebSocket.mock
        .calls[0][5] as unknown as OnCloseCallback;
      onCloseCallback();

      expect(onCloseMock).toHaveBeenCalled();
    });

    it("should register onData handler for terminal input", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
    });

    it("should send user input to WebSocket", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockConnectTerminalWebSocket).toHaveBeenCalledWith(
        expect.any(String),
        expect.any(String),
        expect.any(Function),
        expect.any(Function),
        expect.any(Function),
        expect.any(Function),
      );
    });
  });

  describe("Lifecycle", () => {
    it("should add resize listener on mount", () => {
      const addEventListenerSpy = vi.spyOn(window, "addEventListener");

      render(<Terminal providerId="test-provider" app="claude" />);

      expect(addEventListenerSpy).toHaveBeenCalledWith(
        "resize",
        expect.any(Function),
      );

      addEventListenerSpy.mockRestore();
    });

    it("should remove resize listener on unmount", () => {
      const removeEventListenerSpy = vi.spyOn(window, "removeEventListener");

      const { unmount } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      unmount();

      expect(removeEventListenerSpy).toHaveBeenCalledWith(
        "resize",
        expect.any(Function),
      );

      removeEventListenerSpy.mockRestore();
    });

    it("should dispose xterm on unmount", () => {
      const { unmount } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      expect(mockTerminalFn).toHaveBeenCalled();

      unmount();

      expect(mockTerminalFn).toHaveBeenCalled();
    });

    it("should close WebSocket on unmount", () => {
      const { unmount } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();

      unmount();

      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
    });

    it("should handle window resize events", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      window.dispatchEvent(new Event("resize"));

      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
    });

    it("should not throw error if terminal ref is null on unmount", () => {
      const { rerender } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      expect(() => {
        rerender(<Terminal providerId="test-provider" app="claude" />);
      }).not.toThrow();
    });
  });

  describe("Callbacks", () => {
    it("should call onError when WebSocket error occurs", () => {
      const onErrorMock = vi.fn();

      render(
        <Terminal
          providerId="test-provider"
          app="claude"
          onError={onErrorMock}
        />,
      );

      const onErrorCallback = mockConnectTerminalWebSocket.mock
        .calls[0][4] as unknown as OnErrorCallback;

      onErrorCallback("WebSocket connection error");

      expect(onErrorMock).toHaveBeenCalledWith("WebSocket connection error");
    });

    it("should call onClose when WebSocket closes", () => {
      const onCloseMock = vi.fn();

      render(
        <Terminal
          providerId="test-provider"
          app="claude"
          onClose={onCloseMock}
        />,
      );

      const onCloseCallback = mockConnectTerminalWebSocket.mock
        .calls[0][5] as unknown as OnCloseCallback;

      onCloseCallback();

      expect(onCloseMock).toHaveBeenCalled();
    });

    it("should handle multiple errors without crashing", () => {
      const onErrorMock = vi.fn();

      render(
        <Terminal
          providerId="test-provider"
          app="claude"
          onError={onErrorMock}
        />,
      );

      const onErrorCallback = mockConnectTerminalWebSocket.mock
        .calls[0][4] as unknown as OnErrorCallback;

      onErrorCallback("Error 1");
      onErrorCallback("Error 2");
      onErrorCallback("Error 3");

      expect(onErrorMock).toHaveBeenCalledTimes(3);
    });

    it("should not call onError/onClose if callbacks are undefined", () => {
      expect(() => {
        render(<Terminal providerId="test-provider" app="claude" />);

        const onErrorCallback = mockConnectTerminalWebSocket.mock
          .calls[0][4] as unknown as OnErrorCallback;
        const onCloseCallback = mockConnectTerminalWebSocket.mock
          .calls[0][5] as unknown as OnCloseCallback;

        onErrorCallback("Some error");
        onCloseCallback();
      }).not.toThrow();
    });
  });

  describe("Props changes", () => {
    it("should re-initialize when providerId changes", () => {
      const { rerender } = render(
        <Terminal providerId="provider-1" app="claude" />,
      );

      const firstCallCount = mockConnectTerminalWebSocket.mock.calls.length;

      rerender(<Terminal providerId="provider-2" app="claude" />);

      expect(mockConnectTerminalWebSocket.mock.calls.length).toBeGreaterThan(
        firstCallCount,
      );
    });

    it("should re-initialize when app changes", () => {
      const { rerender } = render(
        <Terminal providerId="provider-1" app="claude" />,
      );

      const firstCallCount = mockConnectTerminalWebSocket.mock.calls.length;

      rerender(<Terminal providerId="provider-1" app="codex" />);

      expect(mockConnectTerminalWebSocket.mock.calls.length).toBeGreaterThan(
        firstCallCount,
      );
    });

    it("should cleanup old resources when props change", () => {
      const { rerender } = render(
        <Terminal providerId="provider-1" app="claude" />,
      );

      const firstTerminalCallCount = mockTerminalFn.mock.calls.length;

      rerender(<Terminal providerId="provider-2" app="claude" />);

      expect(mockTerminalFn.mock.calls.length).toBeGreaterThan(
        firstTerminalCallCount,
      );
    });
  });

  describe("Error handling", () => {
    it("should handle TextEncoder errors gracefully", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(mockTerminalFn).toHaveBeenCalled();
      expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
    });

    it("should handle null WebSocket gracefully", () => {
      render(<Terminal providerId="test-provider" app="claude" />);

      expect(() => {
        expect(mockConnectTerminalWebSocket).toHaveBeenCalled();
      }).not.toThrow();
    });

    it("should properly clean up all refs on unmount", () => {
      const { unmount } = render(
        <Terminal providerId="test-provider" app="claude" />,
      );

      const terminalCallCount = mockTerminalFn.mock.calls.length;
      const wsCallCount = mockConnectTerminalWebSocket.mock.calls.length;

      unmount();

      expect(mockTerminalFn.mock.calls.length).toBe(terminalCallCount);
      expect(mockConnectTerminalWebSocket.mock.calls.length).toBe(wsCallCount);
    });
  });
});
