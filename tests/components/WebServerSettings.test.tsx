import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { WebServerSettings } from "@/components/settings/WebServerSettings";

const originalLocation = window.location;

describe("WebServerSettings", () => {
  beforeEach(() => {
    // @ts-ignore - Simulate web mode
    delete window.__TAURI__;
    // @ts-ignore - Provide a stable web URL
    delete window.location;
    (window as any).location = {
      ...originalLocation,
      href: "http://localhost:13001/",
      host: "localhost:13001",
      hostname: "localhost",
      port: "13001",
      protocol: "http:",
    };
  });

  afterEach(() => {
    (window as any).location = originalLocation;
    vi.restoreAllMocks();
  });

  it("shows running status and current URL in web mode", () => {
    render(<WebServerSettings />);

    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:13001/")).toBeInTheDocument();
  });

  it("does not show desktop-only warning in web mode", () => {
    render(<WebServerSettings />);

    expect(screen.queryByText(/desktop mode only/i)).not.toBeInTheDocument();
  });

  it("does not show start/stop controls in web mode", () => {
    render(<WebServerSettings />);

    expect(screen.queryByText("Start Server")).not.toBeInTheDocument();
    expect(screen.queryByText("Stop Server")).not.toBeInTheDocument();
  });
});
