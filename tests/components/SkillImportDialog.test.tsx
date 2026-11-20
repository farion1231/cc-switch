import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { SkillImportDialog } from "@/components/skills/SkillImportDialog";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

it("shows locked apps as active but does not submit their database switches", async () => {
  const onImport = vi.fn();
  render(
    <SkillImportDialog
      skills={[
        {
          directory: "example",
          name: "Example",
          path: "/skills/example",
          foundIn: ["codex"],
        },
      ]}
      visibleAppIds={["claude", "codex"]}
      lockedApps={["codex"]}
      isImporting={false}
      onImport={onImport}
      onClose={vi.fn()}
    />,
  );
  expect(screen.getByRole("checkbox", { name: /Codex/ })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: /Codex/ })).toBeDisabled();
  await userEvent.click(
    screen.getByRole("button", { name: "skillsPage.import.submit" }),
  );
  expect(onImport).toHaveBeenCalledWith([
    expect.objectContaining({
      apps: expect.objectContaining({ codex: false }),
    }),
  ]);
});
