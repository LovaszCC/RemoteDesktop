import { describe, expect, it } from 'vitest';
import { encodeFrame } from '../testing/rdpFrames';
import { decodeFrame, frameBytes, HEADER_LEN } from './frame';

describe('decodeFrame', () => {
  it('reads the little-endian header and every rectangle', () => {
    const bytes = encodeFrame({
      seq: 0x01020304,
      width: 1280,
      height: 800,
      rects: [
        { x: 10, y: 20, width: 2, height: 3 },
        { x: 300, y: 0, width: 1, height: 1 },
      ],
    });
    expect([...bytes.slice(0, HEADER_LEN)]).toEqual([4, 3, 2, 1, 0, 5, 32, 3, 2, 0, 0, 0]);
    const frame = decodeFrame(bytes)!;
    expect(frame.seq).toBe(0x01020304);
    expect([frame.width, frame.height]).toEqual([1280, 800]);
    expect(frame.rects.map(({ x, y, width, height }) => [x, y, width, height])).toEqual([
      [10, 20, 2, 3],
      [300, 0, 1, 1],
    ]);
    const first = frame.rects[0]!;
    expect(first.pixels.length).toBe(2 * 3 * 4);
    // Row 2, column 1 of rectangle 0.
    expect([...first.pixels.slice((2 * 2 + 1) * 4, (2 * 2 + 2) * 4)]).toEqual([0, 1, 2, 255]);
    expect([...frame.rects[1]!.pixels]).toEqual([1, 0, 0, 255]);
  });

  it('views the message buffer without copying, also at an offset', () => {
    const inner = encodeFrame({ seq: 1, width: 4, height: 4, rects: [{ x: 0, y: 0, width: 1, height: 1 }] });
    const outer = new Uint8Array(inner.length + 5);
    outer.set(inner, 5);
    const frame = decodeFrame(outer.subarray(5))!;
    expect(frame.rects[0]!.pixels.buffer).toBe(outer.buffer);
    expect([...frame.rects[0]!.pixels]).toEqual([0, 0, 0, 255]);
  });

  it('accepts a frame without rectangles', () => {
    expect(decodeFrame(encodeFrame({ seq: 7, width: 800, height: 600, rects: [] }))).toEqual({
      seq: 7,
      width: 800,
      height: 600,
      rects: [],
    });
  });

  it('rejects truncated messages', () => {
    const bytes = encodeFrame({ seq: 1, width: 8, height: 8, rects: [{ x: 0, y: 0, width: 2, height: 2 }] });
    expect(decodeFrame(bytes.subarray(0, 11))).toBeNull();
    expect(decodeFrame(bytes.subarray(0, HEADER_LEN + 4))).toBeNull();
    expect(decodeFrame(bytes.subarray(0, bytes.length - 1))).toBeNull();
  });
});

describe('frameBytes', () => {
  it('accepts what a channel delivers', () => {
    const buffer = new ArrayBuffer(3);
    expect(frameBytes(buffer)?.buffer).toBe(buffer);
    const bytes = new Uint8Array([1]);
    expect(frameBytes(bytes)).toBe(bytes);
    expect([...frameBytes([1, 2])!]).toEqual([1, 2]);
    expect(frameBytes('x')).toBeNull();
  });
});
