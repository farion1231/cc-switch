import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { IpDiagnostics } from "@/components/settings/IpDiagnostics";
import type { IpReport } from "@/lib/api/globalProxy";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  proxy: {
    data: "http://user:secret@proxy.example:8080" as string | null | undefined,
    isLoading: false,
    isError: false,
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => mocks.proxy,
}));

function report(): IpReport {
  return {
    samples: [
      {
        source: "icanhazip.com",
        ip: "203.0.113.10",
        latencyMs: 30,
        error: null,
      },
      { source: "ipify.org", ip: "203.0.113.10", latencyMs: 40, error: null },
    ],
    location: {
      ip: "203.0.113.10",
      country: "Example country",
      region: "Example region",
      city: "Example city",
      connection: { asn: 64500, isp: "Example ISP", org: "Example Org" },
      timezone: { id: "Etc/UTC", utc: "+00:00" },
    },
    locationError: null,
  };
}

const text = (key: string) => `settings.ipDiagnostics.${key}`;
const check = () =>
  fireEvent.click(screen.getByRole("button", { name: text("run") }));
const chooseCustomRoute = async () => {
  const user = userEvent.setup();
  await user.click(screen.getByRole("combobox", { name: text("route") }));
  await user.click(await screen.findByRole("option", { name: text("custom") }));
};

describe("IP diagnostics", () => {
  beforeAll(() => {
    // jsdom has no scrollIntoView, which the open Radix Select calls.
    Element.prototype.scrollIntoView = vi.fn();
  });

  beforeEach(() => {
    mocks.invoke.mockReset().mockResolvedValue(report());
    mocks.proxy.data = "http://user:secret@proxy.example:8080";
    mocks.proxy.isLoading = false;
    mocks.proxy.isError = false;
  });

  it("runs only on demand and displays observations without proxy credentials", async () => {
    const { container } = render(<IpDiagnostics />);
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain("secret");
    expect(screen.queryByText(text("exitScope"))).not.toBeInTheDocument();
    check();
    expect(await screen.findByText("AS64500")).toBeInTheDocument();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledWith("diagnose_ip", {
      url: mocks.proxy.data,
    });
    for (const value of [
      "Example country",
      "Example region",
      "Example city",
      "Example ISP",
      "Example Org",
      "+00:00",
    ]) {
      expect(screen.getByText(value)).toBeInTheDocument();
    }
    expect(screen.getByText(text("consistent"))).toBeInTheDocument();
    expect(screen.getByText(text("exitScope"))).toBeInTheDocument();
    expect(container.textContent).not.toContain("secret");
  });

  it("uses system behavior when the saved proxy is empty", async () => {
    mocks.proxy.data = null;
    render(<IpDiagnostics />);
    expect(screen.getByText(text("systemRoute"))).toBeInTheDocument();
    check();
    await screen.findByText(text("consistent"));
    expect(mocks.invoke).toHaveBeenCalledWith("diagnose_ip", { url: "" });
  });

  it("does not silently run with missing saved settings", async () => {
    mocks.proxy.data = undefined;
    mocks.proxy.isLoading = true;
    const view = render(<IpDiagnostics />);
    expect(screen.getByRole("button", { name: text("run") })).toBeDisabled();
    mocks.proxy.isLoading = false;
    mocks.proxy.isError = true;
    view.rerender(<IpDiagnostics />);
    expect(screen.getByRole("alert")).toHaveTextContent(text("settingsError"));
    expect(screen.getByRole("button", { name: text("run") })).toBeDisabled();
    await chooseCustomRoute();
    expect(screen.getByRole("button", { name: text("run") })).toBeEnabled();
  });

  it("uses the temporary proxy without writing settings", async () => {
    render(<IpDiagnostics />);
    await chooseCustomRoute();
    const input = screen.getByLabelText(text("proxyUrl"));
    expect(input).toHaveAttribute("type", "password");
    fireEvent.change(input, {
      target: { value: "socks5h://temporary.example:1080" },
    });
    check();
    await screen.findByText(text("consistent"));
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledWith("diagnose_ip", {
      url: "socks5h://temporary.example:1080",
    });
    fireEvent.change(input, { target: { value: "" } });
    expect(screen.queryByText(text("consistent"))).not.toBeInTheDocument();
  });

  it("discards a pending result when the selected route changes", async () => {
    let finish!: (value: IpReport) => void;
    mocks.invoke.mockReturnValue(
      new Promise<IpReport>((resolve) => {
        finish = resolve;
      }),
    );
    render(<IpDiagnostics />);
    check();
    expect(
      screen.getByRole("button", { name: text("running") }),
    ).toBeDisabled();
    await chooseCustomRoute();
    await act(async () => {
      finish(report());
    });
    expect(screen.queryByText("AS64500")).not.toBeInTheDocument();
  });

  it("clears stale results when the saved proxy changes", async () => {
    const view = render(<IpDiagnostics />);
    check();
    await screen.findByText("AS64500");
    mocks.proxy.data = "http://different.example:8080";
    view.rerender(<IpDiagnostics />);
    expect(screen.queryByText("AS64500")).not.toBeInTheDocument();
  });

  it("shows different exits without claiming a leak", async () => {
    const result = report();
    result.samples[1].ip = "203.0.113.11";
    mocks.invoke.mockResolvedValue(result);
    render(<IpDiagnostics />);
    check();
    expect(await screen.findByText(text("different"))).toBeInTheDocument();
  });

  it("keeps successful samples when location and another sample fail", async () => {
    const result = report();
    result.samples[0] = { ...result.samples[0], ip: null, error: "network" };
    result.location = null;
    result.locationError = "http";
    mocks.invoke.mockResolvedValue(result);
    render(<IpDiagnostics />);
    check();
    expect(await screen.findByText(text("incomplete"))).toBeInTheDocument();
    expect(screen.getByText(text("errors.network"))).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(text("errors.http"));
    expect(screen.getByText("203.0.113.10 (40 ms)")).toBeInTheDocument();
  });

  it("does not render missing metadata as a successful classification", async () => {
    const result = report();
    result.location = {
      ip: "203.0.113.10",
      country: null,
      region: null,
      city: null,
      connection: null,
      timezone: null,
    };
    result.samples[0] = { ...result.samples[0], ip: null, error: null };
    mocks.invoke.mockResolvedValue(result);
    render(<IpDiagnostics />);
    check();
    expect(
      await screen.findByText(text("errors.unavailable")),
    ).toBeInTheDocument();
    expect(screen.getAllByText(text("unknown")).length).toBeGreaterThan(0);
    expect(screen.queryByText("AS0")).not.toBeInTheDocument();
  });

  it.each(["invalidProxy", "internal error with secret"])(
    "handles command error %s and allows retry",
    async (reason) => {
      mocks.invoke.mockRejectedValueOnce(reason);
      render(<IpDiagnostics />);
      check();
      expect(await screen.findByRole("alert")).toHaveTextContent(
        text(reason === "invalidProxy" ? "invalidProxy" : "failed"),
      );
      expect(screen.queryByText(/secret/)).not.toBeInTheDocument();
      check();
      await screen.findByText("AS64500");
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    },
  );

  it("shows unknown when the runtime cannot report a local timezone", async () => {
    const options = new Intl.DateTimeFormat().resolvedOptions();
    const formatter = vi.spyOn(Intl, "DateTimeFormat").mockReturnValue({
      resolvedOptions: () => ({ ...options, timeZone: "" }),
    } as Intl.DateTimeFormat);
    try {
      render(<IpDiagnostics />);
      check();
      expect(await screen.findByText(text("unknown"))).toBeInTheDocument();
    } finally {
      formatter.mockRestore();
    }
  });
});
