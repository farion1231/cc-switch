import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SkillReconciliationDialog } from "@/components/skills/SkillReconciliationDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("SkillReconciliationDialog", () => {
  it("displays the agents source as its recognizable directory", () => {
    render(
      <SkillReconciliationDialog
        candidates={[
          {
            app: "agents",
            directory: "example",
            name: "Example",
            path: "/home/user/.agents/skills/example",
          },
        ]}
        isRemoving={false}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByText("~/.agents/skills", { exact: false }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("agents", { exact: true }),
    ).not.toBeInTheDocument();
  });

  it("requires explicit selection before removing a duplicate", async () => {
    const user = userEvent.setup();
    const candidate = {
      app: "opencode",
      directory: "example",
      name: "Example",
      path: "/apps/opencode/skills/example",
    };
    const onRemove = vi.fn();
    render(
      <SkillReconciliationDialog
        candidates={[candidate]}
        isRemoving={false}
        onRemove={onRemove}
        onClose={vi.fn()}
      />,
    );
    const remove = screen.getByRole("button", {
      name: "skillsPage.reconciliation.remove",
    });
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(remove).toBeDisabled();
    await user.click(remove);
    expect(onRemove).not.toHaveBeenCalled();
    await user.click(screen.getByRole("checkbox"));
    await user.click(remove);
    expect(onRemove).toHaveBeenCalledWith([candidate]);
  });
});
