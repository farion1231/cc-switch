const reportFrontendError = vi.hoisted(() => vi.fn());

vi.mock("@/lib/frontendLogger", () => ({
  reportFrontendError,
}));

import { render, screen } from "@testing-library/react";
import { FrontendErrorBoundary } from "@/components/FrontendErrorBoundary";

function ThrowingChild(): React.ReactNode {
  throw new Error("sensitive render failure");
}

describe("FrontendErrorBoundary", () => {
  it("reports render failures and replaces the broken tree", () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);

    render(
      <FrontendErrorBoundary>
        <ThrowingChild />
      </FrontendErrorBoundary>,
    );

    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("button")).toBeInTheDocument();
    expect(reportFrontendError).toHaveBeenCalledWith(
      "react.error_boundary",
      expect.objectContaining({ message: "sensitive render failure" }),
      expect.any(String),
    );

    consoleError.mockRestore();
  });
});
