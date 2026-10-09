import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClaudeHistorySettings } from "@/components/settings/ClaudeHistorySettings";
import { settingsApi, type ClaudeHistoryRetention } from "@/lib/api/settings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getClaudeHistoryRetention: vi.fn(),
    setClaudeHistoryRetention: vi.fn(),
  },
}));

const get = vi.mocked(settingsApi.getClaudeHistoryRetention);
const set = vi.mocked(settingsApi.setClaudeHistoryRetention);
const saved = (days: number | null): ClaudeHistoryRetention => ({
  configPath: "/test/claude/settings.json",
  days,
});
const input = () => screen.getByRole("spinbutton");
const save = () => screen.getByRole("button", { name: /^common.sav/ });
const reset = () =>
  screen.getByRole("button", { name: "settings.claudeHistoryRetention.reset" });

beforeEach(() => {
  get.mockReset().mockResolvedValue(saved(730));
  set.mockReset().mockImplementation(async (_, days) => saved(days));
});

describe("ClaudeHistorySettings", () => {
  it.each([90, 730, 99999])(
    "preserves %i days on mount and unrelated rerenders",
    async (days) => {
      get.mockResolvedValue(saved(days));
      const { rerender } = render(<ClaudeHistorySettings />);
      await waitFor(() => expect(input()).toHaveValue(days));
      rerender(<ClaudeHistorySettings />);
      expect(
        screen.queryByRole("button", { name: /^common.sav/ }),
      ).not.toBeInTheDocument();
      expect(set).not.toHaveBeenCalled();
      expect(get).toHaveBeenCalledTimes(1);
    },
  );

  it("uses the 30-day default without creating a configuration", async () => {
    get.mockResolvedValue(saved(null));
    render(<ClaudeHistorySettings />);
    await waitFor(() => expect(input()).toHaveValue(30));
    expect(
      screen.queryByRole("button", { name: /^common.sav/ }),
    ).not.toBeInTheDocument();
    expect(reset()).toBeDisabled();
    expect(set).not.toHaveBeenCalled();
  });

  it("does not allow editing before the initial read resolves", async () => {
    let resolve!: (value: ClaudeHistoryRetention) => void;
    get.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    render(<ClaudeHistorySettings />);
    expect(input()).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: /^common.sav/ }),
    ).not.toBeInTheDocument();
    expect(reset()).toBeDisabled();
    await act(async () => resolve(saved(90)));
    expect(input()).toHaveValue(90);
    expect(set).not.toHaveBeenCalled();
  });

  it("writes the exact requested value only after explicit save", async () => {
    render(<ClaudeHistorySettings />);
    await waitFor(() => expect(input()).toHaveValue(730));
    fireEvent.change(input(), { target: { value: "90" } });
    expect(set).not.toHaveBeenCalled();
    fireEvent.click(save());
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /^common.sav/ }),
      ).not.toBeInTheDocument(),
    );
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith(saved(730), 90);
    expect(input()).toHaveValue(90);
  });

  it("removes the field only after explicit reset", async () => {
    render(<ClaudeHistorySettings />);
    await waitFor(() => expect(input()).toHaveValue(730));
    fireEvent.click(reset());
    await waitFor(() => expect(input()).toHaveValue(30));
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith(saved(730), null);
    expect(reset()).toBeDisabled();
  });

  it.each(["", "0", "-1", "1.5", "9007199254740992"])(
    "rejects invalid input %s",
    async (value) => {
      render(<ClaudeHistorySettings />);
      await waitFor(() => expect(input()).toHaveValue(730));
      fireEvent.change(input(), { target: { value } });
      expect(save()).toBeDisabled();
      expect(screen.getByRole("alert")).toBeInTheDocument();
      expect(set).not.toHaveBeenCalled();
    },
  );

  it("serializes rapid repeated save and reset clicks", async () => {
    let resolve!: (value: ClaudeHistoryRetention) => void;
    set.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    render(<ClaudeHistorySettings />);
    await waitFor(() => expect(input()).toHaveValue(730));
    fireEvent.change(input(), { target: { value: "99999" } });
    fireEvent.click(save());
    fireEvent.click(save());
    fireEvent.click(reset());
    expect(set).toHaveBeenCalledTimes(1);
    expect(input()).toBeDisabled();
    await act(async () => resolve(saved(99999)));
    expect(input()).toHaveValue(99999);
  });

  it("reports read failure, disables writes, and can reload", async () => {
    get.mockRejectedValueOnce(new Error("invalid JSON"));
    render(<ClaudeHistorySettings />);
    await screen.findByRole("alert");
    expect(input()).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: /^common.sav/ }),
    ).not.toBeInTheDocument();
    expect(reset()).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(input()).toHaveValue(730));
    expect(set).not.toHaveBeenCalled();
  });

  it("keeps failed edits visible and reloads an externally changed policy", async () => {
    set.mockRejectedValueOnce(new Error("conflict"));
    render(<ClaudeHistorySettings />);
    await waitFor(() => expect(input()).toHaveValue(730));
    fireEvent.change(input(), { target: { value: "90" } });
    fireEvent.click(save());
    await screen.findByRole("alert");
    expect(input()).toHaveValue(90);
    get.mockResolvedValueOnce(saved(365));
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await waitFor(() => expect(input()).toHaveValue(365));
    fireEvent.change(input(), { target: { value: "90" } });
    fireEvent.click(save());
    await waitFor(() => expect(set).toHaveBeenLastCalledWith(saved(365), 90));
  });

  it("ignores a read that resolves after the component is unmounted", async () => {
    let resolve!: (value: ClaudeHistoryRetention) => void;
    get.mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const { unmount } = render(<ClaudeHistorySettings />);
    unmount();
    await act(async () => resolve(saved(90)));
    expect(set).not.toHaveBeenCalled();
  });
});
