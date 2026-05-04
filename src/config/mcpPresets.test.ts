import { describe, expect, it } from "vitest";
import { createNpxCommandForServerOs } from "./mcpPresets";

describe("createNpxCommandForServerOs", () => {
  it("uses cmd wrapper only when the backend server is Windows", () => {
    expect(createNpxCommandForServerOs("windows", "pkg", ["-y"])).toEqual({
      command: "cmd",
      args: ["/c", "npx", "-y", "pkg"],
    });

    expect(createNpxCommandForServerOs("linux", "pkg", ["-y"])).toEqual({
      command: "npx",
      args: ["-y", "pkg"],
    });
  });

  it("does not use the browser platform when building server commands", () => {
    expect(createNpxCommandForServerOs("macos", "pkg")).toEqual({
      command: "npx",
      args: ["pkg"],
    });
  });
});
