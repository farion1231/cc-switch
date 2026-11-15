import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AutoUpdatePrompt } from "@/components/AutoUpdatePrompt";

const update = vi.hoisted(() => ({
  readyUpdate: null as { version: string } | null,
  routingInUse: false,
  refreshAutoUpdate: vi.fn(async () => {}),
  resetDismiss: vi.fn(),
}));

const api = vi.hoisted(() => ({
  installUpdateAndRestart: vi.fn(async () => true),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { version?: string }) =>
      options?.version ? `${key}:${options.version}` : key,
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => update,
}));

vi.mock("@/lib/api", () => ({
  settingsApi: api,
}));

const DISMISSED_KEY = "ccswitch:autoUpdate:promptDismissedVersion";

describe("AutoUpdatePrompt", () => {
  beforeEach(() => {
    localStorage.clear();
    update.readyUpdate = null;
    update.routingInUse = false;
    api.installUpdateAndRestart.mockClear();
    update.refreshAutoUpdate.mockClear();
  });

  it("renders nothing until an update has been downloaded", () => {
    const { container } = render(<AutoUpdatePrompt />);
    expect(container).toBeEmptyDOMElement();
  });

  it("asks before installing and warns when routing is in use", () => {
    update.readyUpdate = { version: "4.1.0" };
    update.routingInUse = true;
    render(<AutoUpdatePrompt />);

    expect(screen.getByText("autoUpdate.readyTitle:4.1.0")).toBeInTheDocument();
    expect(screen.getByText("autoUpdate.readyBodyRouting")).toBeInTheDocument();
    expect(api.installUpdateAndRestart).not.toHaveBeenCalled();
  });

  it("installs only when the user confirms", async () => {
    update.readyUpdate = { version: "4.1.0" };
    render(<AutoUpdatePrompt />);

    await userEvent.click(screen.getByText("autoUpdate.installNow"));
    expect(api.installUpdateAndRestart).toHaveBeenCalledTimes(1);
  });

  it("'Later' hides the prompt for that version and remembers it", async () => {
    update.readyUpdate = { version: "4.1.0" };
    const { container, rerender } = render(<AutoUpdatePrompt />);

    await userEvent.click(screen.getByText("autoUpdate.later"));
    expect(container).toBeEmptyDOMElement();
    expect(localStorage.getItem(DISMISSED_KEY)).toBe("4.1.0");

    // 之后下好了更新的版本，照样再问
    update.readyUpdate = { version: "4.1.1" };
    rerender(<AutoUpdatePrompt />);
    await waitFor(() =>
      expect(
        screen.getByText("autoUpdate.readyTitle:4.1.1"),
      ).toBeInTheDocument(),
    );
  });
});
