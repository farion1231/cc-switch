import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ManagementScopeSettings } from "@/components/settings/ManagementScopeSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

const settingsWithScope = (
  scope: SettingsFormState["managementScope"],
): SettingsFormState =>
  ({
    managementScope: scope,
  }) as SettingsFormState;

describe("ManagementScopeSettings", () => {
  it("turns off every non-provider manager from the providers-only switch", () => {
    const onChange = vi.fn();
    render(
      <ManagementScopeSettings
        settings={settingsWithScope({
          mcp: true,
          skills: true,
          sessions: true,
        })}
        onChange={onChange}
      />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.managementScope.providersOnly",
      }),
    );

    expect(onChange).toHaveBeenCalledWith({
      managementScope: { mcp: false, skills: false, sessions: false },
    });
  });

  it("updates one resource without changing the others", () => {
    const onChange = vi.fn();
    render(
      <ManagementScopeSettings
        settings={settingsWithScope({
          mcp: false,
          skills: false,
          sessions: false,
        })}
        onChange={onChange}
      />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.managementScope.skills",
      }),
    );

    expect(onChange).toHaveBeenCalledWith({
      managementScope: { mcp: false, skills: true, sessions: false },
    });
  });
});
