import { describe, expect, it } from "vitest";
import { APP_IDS, APP_SPECS } from "@/config/appSpecs";
import backendSpecs from "./appSpecs.json";

// appSpecs.json 由后端测试 app_specs_match_shared_fixture 从 AppType::spec 生成。
// 前后端都有的几项必须一致，应用和顺序也必须一致。
describe("APP_SPECS matches the backend AppType::spec", () => {
  it("lists the same apps in the same order", () => {
    expect(APP_IDS).toEqual(backendSpecs.map((spec) => spec.id));
  });

  it.each(backendSpecs)("declares the same capabilities for $id", (spec) => {
    const caps = APP_SPECS[spec.id as keyof typeof APP_SPECS];
    expect({
      id: spec.id,
      additive: caps.additive,
      localProxy: caps.localProxy,
      routeMode: caps.routeMode,
      stack: caps.stack,
    }).toEqual(spec);
  });
});
