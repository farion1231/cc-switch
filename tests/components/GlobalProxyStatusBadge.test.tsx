import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  GlobalProxyStatusBadge,
  resolveGlobalProxyBadge,
} from "@/components/settings/GlobalProxyStatusBadge";
import type { UpstreamProxyStatus } from "@/lib/api/globalProxy";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const state = vi.hoisted(() => ({ status: null as unknown }));

vi.mock("@/hooks/useGlobalProxy", () => ({
  useUpstreamProxyStatus: () => ({ data: state.status }),
}));

const base: UpstreamProxyStatus = {
  enabled: false,
  proxyUrl: null,
  followSystemProxy: true,
  mode: "system",
  systemProxyUrl: "http://127.0.0.1:7890",
  systemProxySource: "system",
  systemProxyReachable: true,
  systemProxyChanged: false,
  currentSystemProxyUrl: "http://127.0.0.1:7890",
};

describe("resolveGlobalProxyBadge", () => {
  it("marks explicit proxy as the primary badge", () => {
    expect(
      resolveGlobalProxyBadge({
        ...base,
        enabled: true,
        proxyUrl: "http://127.0.0.1:7890",
        mode: "explicit",
      }),
    ).toEqual({
      variant: "default",
      labelKey: "settings.advanced.globalProxy.badgeExplicit",
    });
  });

  it("shows system proxy when reachable and unreachable otherwise", () => {
    expect(resolveGlobalProxyBadge(base)).toEqual({
      variant: "secondary",
      labelKey: "settings.advanced.globalProxy.badgeSystem",
    });
    expect(
      resolveGlobalProxyBadge({ ...base, systemProxyReachable: false }),
    ).toEqual({
      variant: "destructive",
      labelKey: "settings.advanced.globalProxy.badgeUnreachable",
    });
  });

  it("keeps the system badge when reachability is unknown", () => {
    expect(
      resolveGlobalProxyBadge({ ...base, systemProxyReachable: null }),
    ).toEqual({
      variant: "secondary",
      labelKey: "settings.advanced.globalProxy.badgeSystem",
    });
  });

  it("falls back to direct when nothing is in effect", () => {
    expect(resolveGlobalProxyBadge({ ...base, systemProxyUrl: null })).toEqual({
      variant: "outline",
      labelKey: "settings.advanced.globalProxy.badgeDirect",
    });
    expect(
      resolveGlobalProxyBadge({
        ...base,
        mode: "direct",
        systemProxyUrl: null,
      }),
    ).toEqual({
      variant: "outline",
      labelKey: "settings.advanced.globalProxy.badgeDirect",
    });
  });
});

describe("GlobalProxyStatusBadge", () => {
  it("renders nothing until the status is loaded", () => {
    state.status = null;
    const { container } = render(<GlobalProxyStatusBadge />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the resolved label", () => {
    state.status = { ...base, systemProxyReachable: false };
    render(<GlobalProxyStatusBadge />);
    expect(
      screen.getByText("settings.advanced.globalProxy.badgeUnreachable"),
    ).toBeInTheDocument();
  });
});
