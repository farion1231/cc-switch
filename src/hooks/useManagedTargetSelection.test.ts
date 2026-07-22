import { describe, expect, it } from "vitest";

import { chooseManagedTargetId } from "@/hooks/useManagedTargetSelection";
import type { ManagedTarget } from "@/types";

function target(
  id: string,
  kind: ManagedTarget["kind"],
  managementState: ManagedTarget["managementState"] = "managed",
): ManagedTarget {
  return {
    id,
    app: "codex",
    name: id,
    kind,
    configLocation: { path: id },
    managementState,
  };
}

describe("chooseManagedTargetId", () => {
  const windows = target("windows", { type: "localWindows" });
  const wsl = target("wsl", {
    type: "wsl",
    distro: "Ubuntu-24.04",
    user: "m1kasa",
  });

  it("keeps a valid remembered managed target", () => {
    expect(chooseManagedTargetId([windows, wsl], "wsl")).toBe("wsl");
  });

  it("defaults to Windows when the remembered target is unavailable", () => {
    expect(chooseManagedTargetId([wsl, windows], "missing")).toBe("windows");
  });

  it("does not select unmanaged targets", () => {
    const unmanagedWsl = { ...wsl, managementState: "unmanaged" as const };
    expect(chooseManagedTargetId([unmanagedWsl], "wsl")).toBeNull();
  });
});
