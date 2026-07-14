/**
 * jsdom testing shim for `@xyflow/react` (S4 §7 T7, xyflow's own documented recipe —
 * https://reactflow.dev/learn/advanced-use/testing). `<ReactFlow>` measures every node/edge via
 * `ResizeObserver` + `getBoundingClientRect`/`offsetWidth`/`offsetHeight` + `getBBox` (for its
 * internal SVG edge layer) + reads the pane's current zoom scale off a `DOMMatrixReadOnly` built
 * from the transform string — NONE of these exist in jsdom, so mounting a real `<ReactFlow>`
 * under vitest's jsdom environment throws without this shim installed first.
 *
 * Faithful, verbatim port of xyflow's documented Jest recipe (confirmed against the current
 * xyflow docs, 2026-07 — see `sites/reactflow.dev/src/content/learn/advanced-use/testing.mdx`)
 * to this codebase's vitest + TypeScript setup. Call `mockReactFlow()` once per test file
 * (`beforeAll`/`beforeEach`) BEFORE rendering anything that mounts `<ReactFlow>`.
 */

class MockResizeObserver {
  private readonly callback: ResizeObserverCallback;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
  }

  observe(target: Element): void {
    // Real ResizeObserver reports asynchronously — xyflow's own recipe defers via a macrotask so
    // effects that read the just-observed size on the next tick see a value, mirroring real
    // browser timing closely enough for xyflow's internal measurement effects to settle.
    //
    // `contentRect` (final review, S4 D3/D10): added because `nodeRenderers.test.tsx` is the
    // first consumer of this shim to mount a REAL (unmocked) `<ReactFlow>` — `GraphCanvas.test.tsx`
    // stubs `<ReactFlow>` entirely, so xyflow's own internal `XYPanZoom` extent-tracking
    // `ResizeObserver` (which reads `entry.contentRect.width/height` to cache the pane's zoom
    // extent, `@xyflow/system`'s `XYPanZoom`) never actually ran against this mock before. Without
    // `contentRect` here that internal observer's callback throws
    // ("Cannot read properties of undefined (reading 'width')") on the deferred macrotask, which
    // vitest reports as an unhandled exception. `target.getBoundingClientRect()` is what real
    // `ResizeObserverEntry.contentRect` is derived from; jsdom returns an all-zero `DOMRect` (no
    // layout engine) but with every property defined, which is all xyflow's internal code needs.
    setTimeout(() => {
      const contentRect = target.getBoundingClientRect();
      this.callback(
        [{ target, contentRect } as unknown as ResizeObserverEntry],
        this as unknown as ResizeObserver,
      );
    }, 0);
  }

  unobserve(): void {
    // no-op — nothing to release for the stub.
  }

  disconnect(): void {
    // no-op — nothing to release for the stub.
  }
}

class MockDOMMatrixReadOnly {
  m22: number;

  constructor(transform?: string) {
    const scale = transform?.match(/scale\(([1-9.])\)/)?.[1];
    this.m22 = scale !== undefined ? Number(scale) : 1;
  }
}

/** Only run the shim once when requested — matches xyflow's own recipe: re-installing on every
 * `beforeEach` is harmless but pointless once the globals are already patched, and re-running
 * `Object.defineProperties` a second time is a needless (if idempotent) property redefinition. */
let installed = false;

export function mockReactFlow(): void {
  if (installed) return;
  installed = true;

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).ResizeObserver = MockResizeObserver;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).DOMMatrixReadOnly = MockDOMMatrixReadOnly;

  Object.defineProperties(globalThis.HTMLElement.prototype, {
    offsetHeight: {
      configurable: true,
      get(this: HTMLElement) {
        return Number.parseFloat(this.style.height) || 1;
      },
    },
    offsetWidth: {
      configurable: true,
      get(this: HTMLElement) {
        return Number.parseFloat(this.style.width) || 1;
      },
    },
  });

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis.SVGElement.prototype as any).getBBox = () => ({
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
}
