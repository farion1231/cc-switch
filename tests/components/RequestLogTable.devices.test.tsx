import { fireEvent, screen } from "@testing-library/react";
// RequestLogTable formats money through the settings query (useMoneyFormat)
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RequestLogTable } from "@/components/usage/RequestLogTable";
import type { RequestLog } from "@/types/usage";

const useRequestLogsMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

vi.mock("@/lib/query/usage", () => ({
  useRequestLogs: (args: unknown) => useRequestLogsMock(args),
}));

const log = (partial: Partial<RequestLog>): RequestLog => ({
  requestId: "r",
  providerId: "p",
  providerName: "Provider",
  appType: "claude",
  model: "m",
  costMultiplier: "1",
  inputTokens: 1,
  outputTokens: 1,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  inputCostUsd: "0",
  outputCostUsd: "0",
  cacheReadCostUsd: "0",
  cacheCreationCostUsd: "0",
  totalCostUsd: "0.01",
  isStreaming: false,
  latencyMs: 0,
  statusCode: 200,
  createdAt: 1_760_000_000,
  ...partial,
});

describe("RequestLogTable device scope", () => {
  beforeEach(() => {
    useRequestLogsMock.mockReset().mockReturnValue({
      data: {
        data: [
          log({ requestId: "local-1" }),
          log({ requestId: "remote-1", deviceId: "dev-remote" }),
        ],
        total: 2,
        page: 0,
        pageSize: 20,
      },
      isLoading: false,
    });
  });

  it("passes the device scope to the query and shows a device column", () => {
    const onOpenDetail = vi.fn();
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        device="all"
        deviceNames={{ "dev-remote": "Windows PC" }}
        refreshIntervalMs={0}
        onOpenDetail={onOpenDetail}
      />,
    );
    expect(useRequestLogsMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        filters: expect.objectContaining({ device: "all" }),
      }),
    );
    expect(
      screen.getByRole("columnheader", { name: "usageSync.device" }),
    ).toBeInTheDocument();
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("usageSync.thisDevice");
    expect(rows[1]).toHaveTextContent("Windows PC");

    fireEvent.click(rows[0]);
    expect(onOpenDetail).toHaveBeenLastCalledWith("local-1");
    fireEvent.click(rows[1]);
    expect(onOpenDetail).toHaveBeenLastCalledWith("remote-1", "dev-remote");
  });

  it("keeps the original columns for this device", () => {
    render(
      <RequestLogTable range={{ preset: "today" }} refreshIntervalMs={0} />,
    );
    expect(
      screen.queryByRole("columnheader", { name: "usageSync.device" }),
    ).not.toBeInTheDocument();
    expect(screen.getAllByRole("columnheader")).toHaveLength(10);
  });

  it("places the device column after the chosen columns' app (or time) column", () => {
    const headers = () =>
      screen.getAllByRole("columnheader").map((th) => th.textContent);

    const { unmount } = render(
      <RequestLogTable
        range={{ preset: "today" }}
        device="all"
        refreshIntervalMs={0}
        columns={["time", "app", "model", "cost", "statusCode"]}
      />,
    );
    expect(headers()).toEqual([
      "usage.time",
      "usage.app",
      "usageSync.device",
      "usage.model",
      "usage.cost",
      "usage.statusCode",
    ]);
    const cells = screen.getAllByRole("row")[2].querySelectorAll("td");
    expect(cells).toHaveLength(6);
    expect(cells[2]).toHaveTextContent("dev-remo");
    unmount();

    // App column switched off: the device column follows the time column.
    render(
      <RequestLogTable
        range={{ preset: "today" }}
        device="all"
        refreshIntervalMs={0}
        columns={["time", "cost"]}
      />,
    );
    expect(headers()).toEqual(["usage.time", "usageSync.device", "usage.cost"]);
  });
});
