import { describe, expect, it } from "vitest";
import {
  clampDomain,
  domainFromIndexes,
  panDomain,
  zoomDomain,
} from "@/components/usage/trendViewport";

describe("trend viewport math", () => {
  it("zooms around the requested anchor", () => {
    expect(zoomDomain([0, 100], [0, 100], 0.5, 0.25, 1)).toEqual([12.5, 62.5]);
  });

  it("clamps panning to full bounds", () => {
    expect(panDomain([20, 60], [0, 100], -1)).toEqual([0, 40]);
    expect(panDomain([40, 80], [0, 100], 1)).toEqual([60, 100]);
  });

  it("preserves span while clamping", () => {
    expect(clampDomain([-50, 10], [0, 100])).toEqual([0, 60]);
    expect(clampDomain([90, 150], [0, 100])).toEqual([40, 100]);
  });

  it("converts brush indexes to a bucket-inclusive time domain", () => {
    expect(domainFromIndexes([1000, 2000, 3000], 1, 2, [1, 2, 3])).toEqual([
      2000, 6000,
    ]);
  });

  it("handles empty overview data", () => {
    expect(domainFromIndexes([], 0, 0, [])).toBeNull();
  });

  it("inverts scale so a trackpad pinch-out (scale>1) zooms in", () => {
    const full = [0, 1000] as [number, number];
    // The gesture handler passes 1/scale to zoomDomain: fingers apart (scale>1)
    // -> factor<1 -> visible span shrinks (zoom in).
    const zoomedIn = zoomDomain(full, full, 1 / 1.5, 0.5);
    expect(zoomedIn[1] - zoomedIn[0]).toBeLessThan(1000);
    // Pinch in (scale<1) -> factor>1 -> grows back toward full and clamps.
    const zoomedOut = zoomDomain([0, 500], full, 1 / 0.5, 0.5);
    expect(zoomedOut).toEqual([0, 1000]);
  });
});
