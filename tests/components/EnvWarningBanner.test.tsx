import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import type { EnvConflict } from "@/types/env";

const tMock = vi.fn((key: string) => key);

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

const conflicts: EnvConflict[] = [
  {
    varName: "OPENAI_API_KEY",
    varValue: "test-key",
    sourceType: "system",
    sourcePath: "HKEY_CURRENT_USER\\Environment",
  },
];

describe("EnvWarningBanner", () => {
  it("keeps the banner interactive above the window drag region", () => {
    const onDismiss = vi.fn();
    const { container } = render(
      <EnvWarningBanner
        conflicts={conflicts}
        onDismiss={onDismiss}
        onDeleted={vi.fn()}
      />,
    );

    const banner = container.querySelector("[data-tauri-no-drag]");
    expect(banner).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
