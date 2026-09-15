import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RoutingActivationBrand } from "@/components/proxy/RoutingActivationBrand";

describe("RoutingActivationBrand", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("plays a short particle burst only after the current app activates routing", () => {
    vi.useFakeTimers();
    const { rerender } = render(
      <RoutingActivationBrand active={false} contextKey="claude" ready />,
    );

    expect(
      screen.queryByTestId("routing-activation-particles"),
    ).not.toBeInTheDocument();

    rerender(<RoutingActivationBrand active contextKey="claude" ready />);

    expect(
      screen.getByTestId("routing-activation-particles"),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "CC Switch" })).toHaveClass(
      "text-emerald-500",
    );

    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(
      screen.queryByTestId("routing-activation-particles"),
    ).not.toBeInTheDocument();
  });

  it("does not play activation particles when initial status resolves active", () => {
    const { rerender } = render(
      <RoutingActivationBrand
        active={false}
        contextKey="claude"
        ready={false}
      />,
    );

    rerender(<RoutingActivationBrand active contextKey="claude" ready />);

    expect(
      screen.queryByTestId("routing-activation-particles"),
    ).not.toBeInTheDocument();
  });

  it("clears activation particles when switching app context", () => {
    const { rerender } = render(
      <RoutingActivationBrand active={false} contextKey="claude" ready />,
    );

    rerender(<RoutingActivationBrand active contextKey="claude" ready />);

    expect(
      screen.getByTestId("routing-activation-particles"),
    ).toBeInTheDocument();

    rerender(<RoutingActivationBrand active contextKey="codex" ready />);

    expect(
      screen.queryByTestId("routing-activation-particles"),
    ).not.toBeInTheDocument();
  });
});
