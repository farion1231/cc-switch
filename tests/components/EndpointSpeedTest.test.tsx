import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import EndpointSpeedTest from "@/components/providers/forms/EndpointSpeedTest";

const testApiEndpointsMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/vscode", () => ({
  vscodeApi: {
    testApiEndpoints: testApiEndpointsMock,
    getCustomEndpoints: vi.fn().mockResolvedValue([]),
  },
}));

const renderSpeedTest = (proxyUrl?: string) =>
  render(
    <EndpointSpeedTest
      appId="claude"
      value="https://api.example.com"
      onChange={() => undefined}
      initialEndpoints={[{ url: "https://api.example.com" }]}
      onClose={() => undefined}
      autoSelect={false}
      onAutoSelectChange={() => undefined}
      proxyUrl={proxyUrl}
    />,
  );

describe("EndpointSpeedTest provider proxy", () => {
  beforeEach(() => {
    testApiEndpointsMock.mockReset();
    testApiEndpointsMock.mockResolvedValue([
      { url: "https://api.example.com", latency: 42, status: 200 },
    ]);
  });

  it("passes the form's provider proxy to the speed test", async () => {
    renderSpeedTest(" socks5://127.0.0.1:1080 ");

    fireEvent.click(screen.getByText("endpointTest.testSpeed"));

    await waitFor(() => expect(testApiEndpointsMock).toHaveBeenCalled());
    expect(testApiEndpointsMock).toHaveBeenCalledWith(
      ["https://api.example.com"],
      expect.objectContaining({ proxyUrl: "socks5://127.0.0.1:1080" }),
    );
  });

  it("omits a blank provider proxy", async () => {
    renderSpeedTest("   ");

    fireEvent.click(screen.getByText("endpointTest.testSpeed"));

    await waitFor(() => expect(testApiEndpointsMock).toHaveBeenCalled());
    expect(testApiEndpointsMock.mock.calls[0][1].proxyUrl).toBeUndefined();
  });
});
