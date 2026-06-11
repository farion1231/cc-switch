import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WebUiSettings } from "@/components/settings/WebUiSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

const invokeCommandMock = vi.hoisted(() => vi.fn());
const isTauriRuntimeMock = vi.hoisted(() => vi.fn());
const openExternalMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/commandClient", () => ({
  invokeCommand: invokeCommandMock,
  isTauriRuntime: isTauriRuntimeMock,
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    openExternal: openExternalMock,
  },
}));

const baseSettings: SettingsFormState = {
  language: "zh",
  showInTray: true,
  minimizeToTrayOnClose: true,
  webuiEnabled: true,
  webuiHost: "127.0.0.1",
  webuiPort: 15722,
};

const status = {
  running: false,
  enabled: true,
  port: 15722,
  host: "127.0.0.1",
  address: null,
  tokenSet: false,
  authRequired: false,
};

describe("WebUiSettings", () => {
  beforeEach(() => {
    invokeCommandMock.mockReset();
    invokeCommandMock.mockResolvedValue(status);
    isTauriRuntimeMock.mockReset();
    isTauriRuntimeMock.mockReturnValue(true);
    openExternalMock.mockReset();
    openExternalMock.mockResolvedValue(undefined);
  });

  it("enables password editing when the password switch is turned on", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();

    render(<WebUiSettings settings={baseSettings} onChange={onChange} />);

    const switchControl = await screen.findByRole("switch", {
      name: "settings.webui.requirePassword",
    });

    await waitFor(() => expect(switchControl).not.toBeChecked());

    await user.click(switchControl);

    expect(switchControl).toBeChecked();
    expect(onChange).toHaveBeenCalledWith({ webuiToken: "" });
    expect(
      screen.getByPlaceholderText("settings.webui.passwordPlaceholder"),
    ).toBeInTheDocument();
  });

  it("enables password editing in browser WebUI", async () => {
    isTauriRuntimeMock.mockReturnValue(false);
    const user = userEvent.setup();
    const onChange = vi.fn();

    render(<WebUiSettings settings={baseSettings} onChange={onChange} />);

    const switchControl = await screen.findByRole("switch", {
      name: "settings.webui.requirePassword",
    });

    await user.click(switchControl);

    expect(switchControl).toBeChecked();
    expect(onChange).toHaveBeenCalledWith({ webuiToken: "" });
    expect(
      screen.getByPlaceholderText("settings.webui.passwordPlaceholder"),
    ).toBeInTheDocument();
  });

  it("opens the WebUI address through the platform opener", async () => {
    invokeCommandMock.mockResolvedValue({
      ...status,
      running: true,
      address: "http://127.0.0.1:15722",
    });
    const user = userEvent.setup();

    render(<WebUiSettings settings={baseSettings} onChange={vi.fn()} />);

    await screen.findByText("http://127.0.0.1:15722");
    await user.click(screen.getByTitle("settings.webui.openInBrowser"));

    await waitFor(() =>
      expect(openExternalMock).toHaveBeenCalledWith("http://127.0.0.1:15722"),
    );
  });
});
