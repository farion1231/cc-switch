import { describe, expect, it } from "vitest";
import { npmBinCollisionPath } from "@/components/apps/useToolManagement";

describe("npmBinCollisionPath", () => {
  it("extracts the conflicting path from npm's EEXIST output", () => {
    const detail = [
      "npm error code EEXIST",
      "npm error path /home/me/.local/bin/pi",
      "npm error EEXIST: file already exists",
      "npm error File exists: /home/me/.local/bin/pi",
      "npm error Remove the existing file and try again, or run npm",
      "npm error with --force to overwrite files recklessly.",
    ].join("\n");
    expect(npmBinCollisionPath(detail)).toBe("/home/me/.local/bin/pi");
  });

  it("handles Windows paths and CRLF output", () => {
    const detail =
      "npm error code EEXIST\r\nnpm error File exists: C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd\r\n";
    expect(npmBinCollisionPath(detail)).toBe(
      "C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd",
    );
  });

  it("ignores failures that are not a bin collision", () => {
    expect(npmBinCollisionPath("npm error code E404")).toBeNull();
    expect(npmBinCollisionPath("EEXIST without a path")).toBeNull();
  });
});
