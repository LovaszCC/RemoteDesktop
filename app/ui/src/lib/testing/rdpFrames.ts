// Builds RDP frame messages for tests (the inverse of `decodeFrame`).

import { HEADER_LEN, RECT_HEADER_LEN, type Frame, type FrameRect } from '../rdp/frame';

export function encodeFrame(
  frame: Omit<Frame, 'rects'> & { rects: Omit<FrameRect, 'pixels'>[] },
): Uint8Array {
  const size = frame.rects.reduce((sum, r) => sum + RECT_HEADER_LEN + r.width * r.height * 4, HEADER_LEN);
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, frame.seq, true);
  view.setUint16(4, frame.width, true);
  view.setUint16(6, frame.height, true);
  view.setUint16(8, frame.rects.length, true);
  let offset = HEADER_LEN;
  for (const [index, r] of frame.rects.entries()) {
    view.setUint16(offset, r.x, true);
    view.setUint16(offset + 2, r.y, true);
    view.setUint16(offset + 4, r.width, true);
    view.setUint16(offset + 6, r.height, true);
    offset += RECT_HEADER_LEN;
    // Every pixel of rectangle `index` is (index, x, y, 255) so tests can tell them apart.
    for (let py = 0; py < r.height; py += 1) {
      for (let px = 0; px < r.width; px += 1) {
        bytes.set([index, px & 0xff, py & 0xff, 255], offset);
        offset += 4;
      }
    }
  }
  return bytes;
}
