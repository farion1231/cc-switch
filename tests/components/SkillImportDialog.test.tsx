import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { SkillImportDialog } from "@/components/skills/SkillImportDialog";

it("excludes Pi and Omp import toggles and leaves their state to the backend", () => {
  const onImport = vi.fn();
  render(
    <SkillImportDialog
      skills={[
        {
          directory: "native-skill",
          name: "Native skill",
          foundIn: ["claude", "pi", "omp"],
          path: "/skills/native-skill",
        },
      ]}
      visibleAppIds={["claude", "pi", "omp"]}
      isImporting={false}
      onImport={onImport}
      onClose={vi.fn()}
    />,
  );

  expect(screen.getByRole("checkbox", { name: "Claude Code" })).toBeChecked();
  expect(screen.queryByRole("checkbox", { name: "Pi" })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: "Oh My Pi" })).toBeNull();
  fireEvent.click(
    screen.getByRole("button", { name: "skillsPage.import.submit" }),
  );
  expect(onImport).toHaveBeenCalledWith([
    {
      directory: "native-skill",
      apps: expect.objectContaining({ claude: true, pi: false, omp: false }),
    },
  ]);
});
