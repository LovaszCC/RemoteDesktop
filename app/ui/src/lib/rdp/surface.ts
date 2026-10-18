// A session's backing canvas: sized to the remote desktop and painted rectangle by rectangle with
// `putImageData`. It belongs to the session (not to the view), so frames painted while the tab is hidden or
// not mounted yet are kept; the view only scales it to its box.

import type { Frame } from './frame';

export interface RdpSurface {
  /** The canvas the view shows. */
  readonly element: HTMLCanvasElement;
  /** Paints a decoded frame, resizing the canvas to the frame's desktop size first. */
  paint(frame: Frame): void;
  dispose(): void;
}

export type SurfaceFactory = () => RdpSurface;

/** Paints `frame` into a 2D context, resizing `canvas` when the desktop size changed. */
export function paintFrame(
  canvas: { width: number; height: number },
  context: Pick<CanvasRenderingContext2D, 'putImageData' | 'fillRect'> & { fillStyle: unknown },
  frame: Frame,
  makeImage: (pixels: Uint8ClampedArray<ArrayBuffer>, width: number, height: number) => ImageData,
) {
  if (
    frame.width > 0 &&
    frame.height > 0 &&
    (canvas.width !== frame.width || canvas.height !== frame.height)
  ) {
    // Resizing clears the canvas; the server repaints after a resize.
    canvas.width = frame.width;
    canvas.height = frame.height;
    context.fillStyle = '#000';
    context.fillRect(0, 0, frame.width, frame.height);
  }
  for (const rect of frame.rects) {
    if (rect.width === 0 || rect.height === 0) continue;
    context.putImageData(makeImage(rect.pixels, rect.width, rect.height), rect.x, rect.y);
  }
}

export function createCanvasSurface(): RdpSurface {
  const element = document.createElement('canvas');
  element.width = 0;
  element.height = 0;
  const context = element.getContext('2d', { alpha: false });
  return {
    element,
    paint(frame) {
      if (!context) return;
      paintFrame(element, context, frame, (pixels, width, height) => new ImageData(pixels, width, height));
    },
    dispose() {
      element.width = 0;
      element.height = 0;
      element.remove();
    },
  };
}
