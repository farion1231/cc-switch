import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AppVisibilitySettings } from "@/components/settings/AppVisibilitySettings";
import type { SettingsFormState } from "@/hooks/useSettings";

describe("AppVisibilitySettings", () => {
  it("keeps Pi hidden until the user enables it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <AppVisibilitySettings
        settings={{} as SettingsFormState}
        onChange={onChange}
      />,
    );

    await user.click(screen.getByText("apps.pi"));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        visibleApps: expect.objectContaining({ pi: true }),
      }),
    );
  });
});
