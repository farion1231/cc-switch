import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const setActiveTarget = vi.fn();

vi.mock("@/contexts/RuntimeTargetContext", () => ({
  useRuntimeTarget: () => ({
    snapshot: {
      status: "online",
      generation: 2,
      activeTargetId: "prod",
    },
    targets: [{ id: "prod", name: "Production", hostAlias: "prod-api" }],
    setActiveTarget,
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

import { RuntimeTargetSwitcher } from "@/components/remote/RuntimeTargetSwitcher";

describe("RuntimeTargetSwitcher", () => {
  beforeEach(() => setActiveTarget.mockReset());

  it("shows active remote target and can switch back to local", async () => {
    const user = userEvent.setup();
    render(<RuntimeTargetSwitcher onManage={vi.fn()} />);

    await user.click(screen.getByRole("button", { name: /Production/ }));
    await user.click(screen.getByRole("menuitem", { name: /本机/ }));

    expect(setActiveTarget).toHaveBeenCalledWith(undefined);
  });

  it("renders an online status indicator without changing control size", () => {
    render(<RuntimeTargetSwitcher onManage={vi.fn()} />);

    const trigger = screen.getByRole("button", { name: /Production/ });
    expect(trigger).toHaveClass("h-8");
    expect(screen.getByTestId("runtime-status-dot")).toHaveClass(
      "bg-emerald-500",
    );
  });
});
