import { describe, expect, it } from "vitest";
import { encodeQrMatrix, type QrMatrix } from "@/components/settings/QrCodeSvg";

function readFormatCopy1(m: QrMatrix): number {
  const s = m.size;
  const at = (r: number, c: number) => m.modules[r * s + c];
  const bits: number[] = [];
  for (let i = 0; i < 6; i++) bits.push(at(8, i));
  bits.push(at(8, 7), at(8, 8), at(7, 8));
  for (let i = 0; i < 6; i++) bits.push(at(5 - i, 8));
  return bits.reduce((acc, b) => (acc << 1) | b, 0);
}

function readFormatCopy2(m: QrMatrix): number {
  const s = m.size;
  const at = (r: number, c: number) => m.modules[r * s + c];
  const bits: number[] = [];
  for (let i = 0; i < 7; i++) bits.push(at(s - 1 - i, 8));
  for (let i = 7; i >= 0; i--) bits.push(at(8, s - 1 - i));
  return bits.reduce((acc, b) => (acc << 1) | b, 0);
}

describe("QrCodeSvg encoder", () => {
  it("encodes a LAN mobile URL with a 64-char hex token into a valid QR matrix", () => {
    const token = "a".repeat(64);
    const url = `http://192.168.1.42:15722/#t=${token}`;
    const matrix = encodeQrMatrix(url);

    expect(matrix).not.toBeNull();
    expect(matrix!.version).toBeGreaterThanOrEqual(5);
    expect(matrix!.size).toBe(matrix!.version * 4 + 17);
    expect(matrix!.modules.length).toBe(matrix!.size * matrix!.size);

    const at = (r: number, c: number) => matrix!.modules[r * matrix!.size + c];

    // Top-left 7x7 finder pattern outer ring & center 3x3 are dark
    expect(at(0, 0)).toBe(1);
    expect(at(0, 6)).toBe(1);
    expect(at(6, 0)).toBe(1);
    expect(at(3, 3)).toBe(1);
    expect(at(1, 1)).toBe(0);

    // Top-right and bottom-left finder centers
    const s = matrix!.size;
    expect(at(3, s - 4)).toBe(1);
    expect(at(s - 4, 3)).toBe(1);

    // Mandatory dark module at (4 * version + 9, 8)
    expect(at(s - 8, 8)).toBe(1);

    // Format info Copy 1 must match Copy 2 bit-for-bit
    expect(readFormatCopy1(matrix!)).toBe(readFormatCopy2(matrix!));
  });

  it("places matching format info copies and timing-overlapping alignment patterns across V1–V10", () => {
    // Payloads sized to exercise V1, V5, V6, V7, and V10
    for (const payload of [
      "hi",
      `http://192.168.1.10:15722/#t=${"1".repeat(64)}`,
      `http://192.168.100.200:15722/#t=${"b".repeat(88)}`,
      `http://192.168.100.200:15722/#t=${"c".repeat(120)}`,
      `http://192.168.100.200:15722/#t=${"d".repeat(220)}`,
    ]) {
      const m = encodeQrMatrix(payload);
      expect(m).not.toBeNull();
      expect(readFormatCopy1(m!)).toBe(readFormatCopy2(m!));
    }

    // Specifically verify Version 7 alignment patterns at (6, 22), (22, 6), and (22, 22)
    const v7 = encodeQrMatrix("x".repeat(145));
    expect(v7).not.toBeNull();
    expect(v7!.version).toBe(7);
    const at = (r: number, c: number) => v7!.modules[r * v7!.size + c];
    for (const [r, c] of [
      [6, 22],
      [22, 6],
      [22, 22],
    ]) {
      expect(at(r, c)).toBe(1); // center dark
      expect(at(r - 1, c - 1)).toBe(0); // inner white ring
      expect(at(r - 2, c - 2)).toBe(1); // outer dark ring
    }
  });

  it("returns null cleanly for excessively long payloads beyond Version 10 capacity", () => {
    expect(encodeQrMatrix("x".repeat(400))).toBeNull();
  });
});
