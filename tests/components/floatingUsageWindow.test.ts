import { describe, expect, it } from "vitest";
import { clampRestoredPosition } from "@/components/usage/FloatingUsageWindow";

describe("clampRestoredPosition", () => {
  const size = { width: 220, height: 64 };

  it("keeps position when window center is inside a connected monitor", () => {
    const monitor = { x: 0, y: 0, width: 1920, height: 1080 };
    const pos = { x: 100, y: 200 };
    expect(clampRestoredPosition(pos, size, [monitor], monitor, 16)).toEqual(
      pos,
    );
  });

  it("keeps position when center is on a secondary monitor", () => {
    const primary = { x: 0, y: 0, width: 1920, height: 1080 };
    const secondary = { x: 1920, y: 0, width: 1280, height: 1024 };
    const pos = { x: 2000, y: 400 };
    expect(
      clampRestoredPosition(pos, size, [primary, secondary], primary, 16),
    ).toEqual(pos);
  });

  it("clamps to primary monitor when center is off every monitor", () => {
    // 显示器被拔出/布局重排后，旧坐标可能让整窗落到屏外
    const monitor = { x: 0, y: 0, width: 1920, height: 1080 };
    const pos = { x: 20000, y: 20000 };
    expect(clampRestoredPosition(pos, size, [monitor], monitor, 16)).toEqual({
      x: 16,
      y: 16,
    });
  });

  it("clamps to first monitor when primary is null", () => {
    const monitor = { x: 100, y: 100, width: 1920, height: 1080 };
    const pos = { x: -5000, y: -5000 };
    expect(clampRestoredPosition(pos, size, [monitor], null, 16)).toEqual({
      x: 116,
      y: 116,
    });
  });

  it("falls back to origin when no monitor info is available", () => {
    const pos = { x: 20000, y: 20000 };
    expect(clampRestoredPosition(pos, size, [], null, 16)).toEqual({
      x: 16,
      y: 16,
    });
  });
});
