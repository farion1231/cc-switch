import { describe, expect, it } from "vitest";
import { parsePositiveBoundedInt } from "@/components/providers/forms/ProviderForm";

/** ProviderMeta.media_max_images is a Rust u32. */
const UINT32_MAX = 4_294_967_295;

describe("parsePositiveBoundedInt", () => {
  it("parses a plain positive integer", () => {
    expect(parsePositiveBoundedInt("70", UINT32_MAX)).toBe(70);
  });

  it("trims surrounding whitespace", () => {
    expect(parsePositiveBoundedInt("  42  ", UINT32_MAX)).toBe(42);
  });

  it("returns undefined for blanks, zero, and negatives", () => {
    // The form treats all three as "no cap" rather than as an error.
    expect(parsePositiveBoundedInt("", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt("   ", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt("0", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt("-1", UINT32_MAX)).toBeUndefined();
  });

  it("accepts scientific notation instead of truncating it", () => {
    // The input is type=number, so the browser can produce "1e2".
    // parseInt("1e2") would silently store 1 and strip every image past the
    // first; Number() gives the 100 the user actually typed.
    expect(parsePositiveBoundedInt("1e2", UINT32_MAX)).toBe(100);
  });

  it("rejects fractional input rather than rounding it", () => {
    expect(parsePositiveBoundedInt("3.7", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt(".5", UINT32_MAX)).toBeUndefined();
  });

  it("rejects values above the bound so the save cannot fail in serde", () => {
    // media_max_images is Option<u32>; storing 2^32 would make the whole
    // provider save fail with a raw deserialization error.
    expect(parsePositiveBoundedInt(String(UINT32_MAX), UINT32_MAX)).toBe(
      UINT32_MAX,
    );
    expect(
      parsePositiveBoundedInt(String(UINT32_MAX + 1), UINT32_MAX),
    ).toBeUndefined();
    expect(parsePositiveBoundedInt("99999999999999999999", UINT32_MAX)).toBe(
      undefined,
    );
  });

  it("rejects non-numeric text", () => {
    expect(parsePositiveBoundedInt("abc", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt("Infinity", UINT32_MAX)).toBeUndefined();
    expect(parsePositiveBoundedInt("NaN", UINT32_MAX)).toBeUndefined();
  });
});
