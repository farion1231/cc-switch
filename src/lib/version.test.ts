import { describe, it, expect } from "vitest";
import { compareVersions, isUpdateAvailable } from "./version";

describe("compareVersions", () => {
  it("按主版本三段比较大小", () => {
    expect(compareVersions("2.1.156", "2.1.154")).toBeGreaterThan(0);
    expect(compareVersions("2.1.154", "2.1.156")).toBeLessThan(0);
    expect(compareVersions("2.2.0", "2.1.999")).toBeGreaterThan(0);
    expect(compareVersions("3.0.0", "2.9.9")).toBeGreaterThan(0);
    expect(compareVersions("2.1.156", "2.1.156")).toBe(0);
  });

  it("预发布版低于同核心的正式版", () => {
    expect(compareVersions("2.1.156-beta.1", "2.1.156")).toBeLessThan(0);
    expect(compareVersions("2.1.156", "2.1.156-rc.1")).toBeGreaterThan(0);
  });

  it("预发布段之间:数字按数值、数字<非数字、段多者更大", () => {
    expect(compareVersions("1.0.0-beta.2", "1.0.0-beta.11")).toBeLessThan(0);
    expect(compareVersions("1.0.0-alpha", "1.0.0-beta")).toBeLessThan(0);
    expect(compareVersions("1.0.0-beta", "1.0.0-beta.1")).toBeLessThan(0);
  });

  it("无法解析时保守返回 0", () => {
    expect(compareVersions("", "2.1.154")).toBe(0);
    expect(compareVersions("unknown", "2.1.154")).toBe(0);
  });
});

describe("isUpdateAvailable", () => {
  it("仅当 latest 严格高于 current 才提示更新", () => {
    expect(isUpdateAvailable("2.1.154", "2.1.156")).toBe(true);
  });

  it("抢先版反超 latest 时不提示更新(本地 156 > latest 154)", () => {
    // 本场景的核心:Claude Code next 通道 156 高于 npm latest 154
    expect(isUpdateAvailable("2.1.156", "2.1.154")).toBe(false);
  });

  it("版本相等不提示更新", () => {
    expect(isUpdateAvailable("2.1.156", "2.1.156")).toBe(false);
  });

  it("缺少当前版本或最新版本时不提示更新", () => {
    expect(isUpdateAvailable(undefined, "2.1.156")).toBe(false);
    expect(isUpdateAvailable("2.1.156", null)).toBe(false);
    expect(isUpdateAvailable("", "")).toBe(false);
  });
});
