/**
 * Self-contained ISO/IEC 18004 QR Code (Byte mode, ECC Level L, Versions 1–10)
 * rendered as a crisp inline SVG with zero external dependencies.
 *
 * Capacity at ECC-L: up to 271 UTF-8 bytes (a typical LAN pairing URL with a
 * 64-char hex token is ~95–110 bytes, fitting in Version 5–6).
 */

import { useMemo } from "react";

// Total codewords, EC codewords per block, and block counts for Versions 1..10 at ECC-L.
// Format: [version, totalCodewords, ecPerBlock, g1Blocks, g1DataPerBlock, g2Blocks, g2DataPerBlock]
const VERSION_TABLE: ReadonlyArray<
  readonly [number, number, number, number, number, number, number]
> = [
  [1, 26, 7, 1, 19, 0, 0],
  [2, 44, 10, 1, 34, 0, 0],
  [3, 70, 15, 1, 55, 0, 0],
  [4, 100, 20, 1, 80, 0, 0],
  [5, 134, 26, 1, 108, 0, 0],
  [6, 172, 18, 2, 68, 0, 0],
  [7, 196, 20, 2, 78, 0, 0],
  [8, 242, 24, 2, 97, 0, 0],
  [9, 292, 30, 2, 116, 0, 0],
  [10, 346, 18, 2, 68, 2, 69],
];

const ALIGNMENT_CENTERS: Record<number, readonly number[]> = {
  1: [],
  2: [6, 18],
  3: [6, 22],
  4: [6, 26],
  5: [6, 30],
  6: [6, 34],
  7: [6, 22, 38],
  8: [6, 24, 42],
  9: [6, 26, 46],
  10: [6, 28, 50],
};

// Precomputed GF(256) log/exp tables with primitive polynomial 0x11d.
const GF_EXP = new Uint8Array(512);
const GF_LOG = new Uint8Array(256);
(() => {
  let x = 1;
  for (let i = 0; i < 255; i++) {
    GF_EXP[i] = x;
    GF_LOG[x] = i;
    x <<= 1;
    if (x & 0x100) x ^= 0x11d;
  }
  for (let i = 255; i < 512; i++) {
    GF_EXP[i] = GF_EXP[i - 255];
  }
})();

function gfMul(a: number, b: number): number {
  if (a === 0 || b === 0) return 0;
  return GF_EXP[GF_LOG[a] + GF_LOG[b]];
}

function rsGeneratorPoly(degree: number): Uint8Array {
  let poly = new Uint8Array([1]);
  for (let i = 0; i < degree; i++) {
    const next = new Uint8Array(poly.length + 1);
    const factor = GF_EXP[i];
    for (let j = 0; j < poly.length; j++) {
      next[j] ^= poly[j];
      next[j + 1] ^= gfMul(poly[j], factor);
    }
    poly = next;
  }
  return poly;
}

function rsRemainder(data: Uint8Array, ecCount: number): Uint8Array {
  const gen = rsGeneratorPoly(ecCount);
  const rem = new Uint8Array(ecCount);
  for (let i = 0; i < data.length; i++) {
    const factor = data[i] ^ rem[0];
    rem.copyWithin(0, 1);
    rem[ecCount - 1] = 0;
    if (factor !== 0) {
      for (let j = 0; j < ecCount; j++) {
        rem[j] ^= gfMul(gen[j + 1], factor);
      }
    }
  }
  return rem;
}

function appendBits(bits: number[], value: number, length: number) {
  for (let i = length - 1; i >= 0; i--) {
    bits.push((value >>> i) & 1);
  }
}

function formatBitsForMask(mask: number): number {
  // ECC Level L = 01 (2 bits) + 3-bit mask
  const data = (0b01 << 3) | mask;
  let rem = data << 10;
  for (let i = 14; i >= 10; i--) {
    if ((rem >>> i) & 1) {
      rem ^= 0b10100110111 << (i - 10);
    }
  }
  return ((data << 10) | rem) ^ 0b101010000010010;
}

function versionBits(version: number): number {
  let rem = version << 12;
  for (let i = 17; i >= 12; i--) {
    if ((rem >>> i) & 1) {
      rem ^= 0b1111100100101 << (i - 12);
    }
  }
  return (version << 12) | rem;
}

function maskBit(mask: number, r: number, c: number): boolean {
  switch (mask) {
    case 0:
      return (r + c) % 2 === 0;
    case 1:
      return r % 2 === 0;
    case 2:
      return c % 3 === 0;
    case 3:
      return (r + c) % 3 === 0;
    case 4:
      return (Math.floor(r / 2) + Math.floor(c / 3)) % 2 === 0;
    case 5:
      return ((r * c) % 2) + ((r * c) % 3) === 0;
    case 6:
      return (((r * c) % 2) + ((r * c) % 3)) % 2 === 0;
    default:
      return (((r + c) % 2) + ((r * c) % 3)) % 2 === 0;
  }
}

export interface QrMatrix {
  size: number;
  version: number;
  modules: Uint8Array; // 1 = dark, 0 = light, row-major
}

export function encodeQrMatrix(text: string): QrMatrix | null {
  const bytes = new TextEncoder().encode(text);
  const spec = VERSION_TABLE.find(([ver, , , g1b, g1d, g2b, g2d]) => {
    const dataCapacity = g1b * g1d + g2b * g2d;
    const countBits = ver <= 9 ? 8 : 16;
    const neededBytes = Math.ceil((4 + countBits + bytes.length * 8) / 8);
    return neededBytes <= dataCapacity;
  });
  if (!spec) return null;

  const [version, , ecPerBlock, g1Blocks, g1Data, g2Blocks, g2Data] = spec;
  const totalDataBytes = g1Blocks * g1Data + g2Blocks * g2Data;
  const countBits = version <= 9 ? 8 : 16;

  const bits: number[] = [];
  appendBits(bits, 0b0100, 4); // Byte mode
  appendBits(bits, bytes.length, countBits);
  for (const b of bytes) appendBits(bits, b, 8);

  const maxBits = totalDataBytes * 8;
  const terminator = Math.min(4, maxBits - bits.length);
  appendBits(bits, 0, terminator);
  while (bits.length % 8 !== 0) bits.push(0);

  const dataBytes = new Uint8Array(totalDataBytes);
  let byteIdx = 0;
  for (let i = 0; i < bits.length; i += 8) {
    let val = 0;
    for (let j = 0; j < 8; j++) val = (val << 1) | bits[i + j];
    dataBytes[byteIdx++] = val;
  }
  const pads = [0xec, 0x11];
  let padIdx = 0;
  while (byteIdx < totalDataBytes) {
    dataBytes[byteIdx++] = pads[padIdx++ & 1];
  }

  // Split into RS blocks & interleave
  const dataBlocks: Uint8Array[] = [];
  const ecBlocks: Uint8Array[] = [];
  let offset = 0;
  const blockSizes = [
    ...Array<number>(g1Blocks).fill(g1Data),
    ...Array<number>(g2Blocks).fill(g2Data),
  ];
  for (const len of blockSizes) {
    const slice = dataBytes.subarray(offset, offset + len);
    offset += len;
    dataBlocks.push(slice);
    ecBlocks.push(rsRemainder(slice, ecPerBlock));
  }

  const codewords: number[] = [];
  const maxDataLen = Math.max(g1Data, g2Data);
  for (let i = 0; i < maxDataLen; i++) {
    for (const blk of dataBlocks) {
      if (i < blk.length) codewords.push(blk[i]);
    }
  }
  for (let i = 0; i < ecPerBlock; i++) {
    for (const blk of ecBlocks) {
      codewords.push(blk[i]);
    }
  }

  const size = version * 4 + 17;
  const isFunction = new Uint8Array(size * size);
  const base = new Uint8Array(size * size);

  const setFn = (r: number, c: number, dark: boolean) => {
    if (r < 0 || c < 0 || r >= size || c >= size) return;
    isFunction[r * size + c] = 1;
    base[r * size + c] = dark ? 1 : 0;
  };

  // Finder patterns + separators
  const drawFinder = (r0: number, c0: number) => {
    for (let dr = -1; dr <= 7; dr++) {
      for (let dc = -1; dc <= 7; dc++) {
        const inBorder = dr === 0 || dr === 6 || dc === 0 || dc === 6;
        const inCenter = dr >= 2 && dr <= 4 && dc >= 2 && dc <= 4;
        const inBox = dr >= 0 && dr <= 6 && dc >= 0 && dc <= 6;
        setFn(r0 + dr, c0 + dc, inBox && (inBorder || inCenter));
      }
    }
  };
  drawFinder(0, 0);
  drawFinder(0, size - 7);
  drawFinder(size - 7, 0);

  // Timing patterns
  for (let i = 8; i < size - 8; i++) {
    setFn(6, i, i % 2 === 0);
    setFn(i, 6, i % 2 === 0);
  }

  // Alignment patterns — skip only the 3 finder regions (do NOT skip timing-line overlaps
  // such as (6, 22) and (22, 6) in Versions 7–10).
  const centers = ALIGNMENT_CENTERS[version] ?? [];
  for (const r of centers) {
    for (const c of centers) {
      if (
        (r <= 8 && c <= 8) ||
        (r <= 8 && c >= size - 8) ||
        (r >= size - 8 && c <= 8)
      ) {
        continue;
      }
      for (let dr = -2; dr <= 2; dr++) {
        for (let dc = -2; dc <= 2; dc++) {
          const dist = Math.max(Math.abs(dr), Math.abs(dc));
          setFn(r + dr, c + dc, dist !== 1);
        }
      }
    }
  }

  // Reserve format info areas & dark module
  for (let i = 0; i <= 8; i++) {
    if (i !== 6) {
      setFn(8, i, false);
      setFn(i, 8, false);
    }
  }
  for (let i = size - 8; i < size; i++) {
    setFn(8, i, false);
    setFn(i, 8, false);
  }
  setFn(size - 8, 8, true);

  // Version info for Version >= 7
  if (version >= 7) {
    const vBits = versionBits(version);
    for (let i = 0; i < 18; i++) {
      const dark = ((vBits >>> i) & 1) === 1;
      const a = Math.floor(i / 3);
      const b = size - 11 + (i % 3);
      setFn(a, b, dark);
      setFn(b, a, dark);
    }
  }

  // Place data bits in upward/downward zigzag columns
  const dataBits: number[] = [];
  for (const cw of codewords) appendBits(dataBits, cw, 8);

  let bitIdx = 0;
  let upward = true;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let step = 0; step < size; step++) {
      const r = upward ? size - 1 - step : step;
      for (let dx = 0; dx < 2; dx++) {
        const c = right - dx;
        if (!isFunction[r * size + c]) {
          base[r * size + c] =
            bitIdx < dataBits.length ? dataBits[bitIdx++] : 0;
        }
      }
    }
    upward = !upward;
  }

  // Evaluate masks 0..7 and pick lowest penalty
  let bestModules = new Uint8Array(size * size);
  let bestPenalty = Number.POSITIVE_INFINITY;

  for (let mask = 0; mask < 8; mask++) {
    const candidate = new Uint8Array(base);
    for (let r = 0; r < size; r++) {
      for (let c = 0; c < size; c++) {
        const idx = r * size + c;
        if (!isFunction[idx] && maskBit(mask, r, c)) {
          candidate[idx] ^= 1;
        }
      }
    }
    // Write format info
    const fmt = formatBitsForMask(mask);
    for (let i = 0; i < 6; i++)
      candidate[8 * size + i] = (fmt >>> (14 - i)) & 1;
    candidate[8 * size + 7] = (fmt >>> 8) & 1;
    candidate[8 * size + 8] = (fmt >>> 7) & 1;
    candidate[7 * size + 8] = (fmt >>> 6) & 1;
    for (let i = 0; i < 6; i++) {
      candidate[(5 - i) * size + 8] = (fmt >>> (5 - i)) & 1;
    }
    for (let i = 0; i < 8; i++) {
      candidate[8 * size + (size - 1 - i)] = (fmt >>> i) & 1;
    }
    for (let i = 0; i < 7; i++) {
      candidate[(size - 1 - i) * size + 8] = (fmt >>> (14 - i)) & 1;
    }

    const penalty = scorePenalty(candidate, size);
    if (penalty < bestPenalty) {
      bestPenalty = penalty;
      bestModules = candidate;
    }
  }

  return { size, version, modules: bestModules };
}

function scorePenalty(modules: Uint8Array, size: number): number {
  let penalty = 0;
  let darkCount = 0;

  // Rule 1: adjacent runs of 5+ same-colored modules
  for (let r = 0; r < size; r++) {
    let run = 1;
    let prev = modules[r * size];
    if (prev) darkCount++;
    for (let c = 1; c < size; c++) {
      const cur = modules[r * size + c];
      if (cur) darkCount++;
      if (cur === prev) {
        run++;
      } else {
        if (run >= 5) penalty += 3 + (run - 5);
        run = 1;
        prev = cur;
      }
    }
    if (run >= 5) penalty += 3 + (run - 5);
  }
  for (let c = 0; c < size; c++) {
    let run = 1;
    let prev = modules[c];
    for (let r = 1; r < size; r++) {
      const cur = modules[r * size + c];
      if (cur === prev) {
        run++;
      } else {
        if (run >= 5) penalty += 3 + (run - 5);
        run = 1;
        prev = cur;
      }
    }
    if (run >= 5) penalty += 3 + (run - 5);
  }

  // Rule 2: 2x2 blocks
  for (let r = 0; r < size - 1; r++) {
    for (let c = 0; c < size - 1; c++) {
      const v = modules[r * size + c];
      if (
        v === modules[r * size + c + 1] &&
        v === modules[(r + 1) * size + c] &&
        v === modules[(r + 1) * size + c + 1]
      ) {
        penalty += 3;
      }
    }
  }

  // Rule 4: dark/light balance
  const total = size * size;
  const pct = (darkCount * 100) / total;
  penalty += Math.floor(Math.abs(pct - 50) / 5) * 10;

  return penalty;
}

interface QrCodeSvgProps {
  value: string;
  size?: number;
  className?: string;
}

export function QrCodeSvg({ value, size = 160, className }: QrCodeSvgProps) {
  const pathData = useMemo(() => {
    const matrix = encodeQrMatrix(value);
    if (!matrix) return null;
    const quiet = 4; // ISO/IEC 18004 minimum 4-module quiet zone
    const viewSize = matrix.size + quiet * 2;
    const parts: string[] = [];
    for (let r = 0; r < matrix.size; r++) {
      for (let c = 0; c < matrix.size; c++) {
        if (matrix.modules[r * matrix.size + c]) {
          parts.push(`M${c + quiet},${r + quiet}h1v1h-1z`);
        }
      }
    }
    return { viewSize, d: parts.join("") };
  }, [value]);

  if (!pathData) return null;

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${pathData.viewSize} ${pathData.viewSize}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label="QR Code"
      className={className}
    >
      <rect
        width={pathData.viewSize}
        height={pathData.viewSize}
        fill="#ffffff"
        rx="2"
      />
      <path d={pathData.d} fill="#18181b" />
    </svg>
  );
}
