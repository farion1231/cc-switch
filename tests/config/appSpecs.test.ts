import { describe, expect, it } from "vitest";
import { APP_IDS, APP_SPECS, ROUTE_MODE_APP_IDS } from "@/config/appSpecs";
import type { AppCaps } from "@/config/appSpecs";
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

// 路由设置的「使用中」列表按这个顺序，和后端 route_mode_apps 一致，Claude Desktop 在最后。
it("orders route-mode apps with Claude Desktop last", () => {
  expect(ROUTE_MODE_APP_IDS).toEqual([
    "claude",
    "codex",
    "gemini",
    "grokbuild",
    "cursor",
    "claude-desktop",
  ]);
});

// Cursor 经公网路由接入，有路由模式但没有 Stack，也不走累加式写入。
it("declares the Cursor capability set", () => {
  const caps = (APP_SPECS as Record<string, AppCaps | undefined>).cursor;
  expect(caps).toEqual({
    localProxy: true,
    routeMode: true,
    stack: false,
    additive: false,
    skills: true,
    mcp: true,
    prompts: true,
    plugins: false,
    managedAccount: false,
    piFamily: false,
    editorView: false,
    officialSubscription: false,
  });
});
