import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ErrorBoundary } from "@/components/common/ErrorBoundary";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      key === "errorBoundary.title"
        ? "Something went wrong"
        : "A part of this page failed to load.",
  }),
}));

const ThrowError = ({ message }: { message: string }) => {
  throw new Error(message);
};

describe("ErrorBoundary", () => {
  it("renders children when there is no error", () => {
    render(
      <ErrorBoundary>
        <div data-testid="child">Hello</div>
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("child")).toBeInTheDocument();
  });

  it("renders fallback alert when a child throws", () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <ThrowError message="Test crash" />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    consoleSpy.mockRestore();
  });

  it("calls onError callback when a child throws", () => {
    const onError = vi.fn();
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary onError={onError}>
        <ThrowError message="Test crash" />
      </ErrorBoundary>,
    );
    expect(onError).toHaveBeenCalled();
    const [error, errorInfo] = onError.mock.calls[0];
    expect(error).toBeInstanceOf(Error);
    expect(error.message).toBe("Test crash");
    expect(errorInfo).toHaveProperty("componentStack");
    consoleSpy.mockRestore();
  });

  it("renders custom fallback when provided", () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary fallback={<div data-testid="custom-fallback">Custom</div>}>
        <ThrowError message="Test crash" />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("custom-fallback")).toBeInTheDocument();
    consoleSpy.mockRestore();
  });
});
