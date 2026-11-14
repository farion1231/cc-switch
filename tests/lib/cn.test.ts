import { describe, expect, it } from "vitest";
import { cn } from "@/lib/utils";

describe("cn", () => {
  it("keeps a v7 font size next to a v7 text color", () => {
    expect(cn("text-body", "text-fg-1")).toBe("text-body text-fg-1");
  });

  it("lets a v7 font size replace a default one", () => {
    expect(cn("text-sm", "text-body")).toBe("text-body");
  });

  it("treats v7 radius and shadow names as their own groups", () => {
    expect(cn("rounded-lg", "rounded-control")).toBe("rounded-control");
    expect(cn("shadow-sm", "shadow-v7-md")).toBe("shadow-v7-md");
  });
});
