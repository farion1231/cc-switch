import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ManagedTargetSelector } from "@/components/providers/ManagedTargetSelector";

describe("ManagedTargetSelector", () => {
  it("shows a recoverable error state when targets cannot be loaded", async () => {
    const onManage = vi.fn();

    render(
      <ManagedTargetSelector
        targets={[]}
        selectedTargetId={null}
        onSelect={vi.fn()}
        onManage={onManage}
        error="backend unavailable"
      />,
    );

    expect(screen.getByText(/backend unavailable/)).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", {
        name: "settings.environments.manage",
      }),
    );
    expect(onManage).toHaveBeenCalledOnce();
  });
});
