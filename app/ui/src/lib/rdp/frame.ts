// The binary frames of an embedded RDP session (`rd_rdp::frame`, little endian): a 12-byte header
// `seq u32, width u16, height u16, count u16, 0 u16`, then per rectangle `x, y, w, h` (u16) followed by
// its RGBA rows, top to bottom.

export const HEADER_LEN = 12;
export const RECT_HEADER_LEN = 8;

/** One dirty rectangle; `pixels` views the message buffer (no copy). */
export interface FrameRect {
  x: number;
  y: number;
  width: number;
  height: number;
  pixels: Uint8ClampedArray<ArrayBuffer>;
}

export interface Frame {
  seq: number;
  /** The desktop size (the canvas size). */
  width: number;
  height: number;
  rects: FrameRect[];
}

/** Raw channel data as bytes (a Tauri channel delivers an `ArrayBuffer`). */
export function frameBytes(data: unknown): Uint8Array | null {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (Array.isArray(data)) return Uint8Array.from(data as number[]);
  return null;
}

/** Decodes a frame message; `null` when it is truncated or malformed. */
export function decodeFrame(bytes: Uint8Array): Frame | null {
  if (bytes.byteLength < HEADER_LEN) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const seq = view.getUint32(0, true);
  const width = view.getUint16(4, true);
  const height = view.getUint16(6, true);
  const count = view.getUint16(8, true);
  const rects: FrameRect[] = [];
  let offset = HEADER_LEN;
  for (let i = 0; i < count; i += 1) {
    if (offset + RECT_HEADER_LEN > bytes.byteLength) return null;
    const x = view.getUint16(offset, true);
    const y = view.getUint16(offset + 2, true);
    const w = view.getUint16(offset + 4, true);
    const h = view.getUint16(offset + 6, true);
    offset += RECT_HEADER_LEN;
    const length = w * h * 4;
    if (offset + length > bytes.byteLength) return null;
    rects.push({
      x,
      y,
      width: w,
      height: h,
      pixels: new Uint8ClampedArray(bytes.buffer as ArrayBuffer, bytes.byteOffset + offset, length),
    });
    offset += length;
  }
  return { seq, width, height, rects };
}
