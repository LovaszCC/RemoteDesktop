import { describe, expect, it, vi } from 'vitest';
import { decodeFrame } from './frame';
import { encodeFrame } from '../testing/rdpFrames';
import { paintFrame } from './surface';

function fakeContext() {
  return { putImageData: vi.fn(), fillRect: vi.fn(), fillStyle: '' as unknown };
}

const image = (pixels: Uint8ClampedArray<ArrayBuffer>, width: number, height: number) =>
  ({ data: pixels, width, height }) as unknown as ImageData;

describe('paintFrame', () => {
  it('sizes the canvas to the desktop and puts every rectangle at its place', () => {
    const canvas = { width: 0, height: 0 };
    const context = fakeContext();
    const frame = decodeFrame(
      encodeFrame({
        seq: 1,
        width: 640,
        height: 480,
        rects: [
          { x: 5, y: 6, width: 2, height: 2 },
          { x: 0, y: 0, width: 0, height: 3 },
        ],
      }),
    )!;
    paintFrame(canvas, context, frame, image);
    expect(canvas).toEqual({ width: 640, height: 480 });
    expect(context.fillRect).toHaveBeenCalledWith(0, 0, 640, 480);
    expect(context.putImageData).toHaveBeenCalledTimes(1);
    const [img, x, y] = context.putImageData.mock.calls[0]!;
    expect([x, y, img.width, img.height, img.data.length]).toEqual([5, 6, 2, 2, 16]);
  });

  it('keeps the canvas when the size is unchanged', () => {
    const canvas = { width: 640, height: 480 };
    const context = fakeContext();
    paintFrame(
      canvas,
      context,
      decodeFrame(encodeFrame({ seq: 2, width: 640, height: 480, rects: [] }))!,
      image,
    );
    expect(context.fillRect).not.toHaveBeenCalled();
  });
});
