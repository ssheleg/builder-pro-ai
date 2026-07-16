// @vitest-environment jsdom
import { describe, it, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useSubmitGuard } from "./useSubmitGuard";

/** A promise whose settle time this test controls — lets us observe the `submitting` flag WHILE the
 * wrapped call is in flight (between invocation and settle), which is the whole point of the guard. */
function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useSubmitGuard", () => {
  it("calls the wrapped fn ONCE when the guarded handler fires twice synchronously (double-fire)", async () => {
    const d = deferred<void>();
    const fn = vi.fn(() => d.promise);
    const { result } = renderHook(() => useSubmitGuard());

    const guarded = result.current.guard(fn);

    // Two synchronous invocations before the first settles — two rapid clicks / Enters in one tick,
    // the exact P-19 race. The synchronous in-flight lock (a ref, not the async state) must block
    // the second call.
    await act(async () => {
      void guarded();
      void guarded();
    });

    expect(fn).toHaveBeenCalledTimes(1);

    await act(async () => {
      d.resolve();
      await d.promise;
    });
  });

  it("flips submitting TRUE during the await and FALSE after it settles", async () => {
    const d = deferred<void>();
    const fn = vi.fn(() => d.promise);
    const { result } = renderHook(() => useSubmitGuard());

    expect(result.current.submitting).toBe(false);

    await act(async () => {
      void result.current.guard(fn)();
    });
    expect(result.current.submitting).toBe(true);

    await act(async () => {
      d.resolve();
      await d.promise;
    });
    expect(result.current.submitting).toBe(false);
  });

  it("resets submitting to FALSE even when the wrapped fn rejects", async () => {
    const d = deferred<void>();
    const fn = vi.fn(() => d.promise);
    const { result } = renderHook(() => useSubmitGuard());

    let call!: Promise<void>;
    await act(async () => {
      call = result.current.guard(fn)();
    });
    expect(result.current.submitting).toBe(true);

    await act(async () => {
      d.reject(new Error("boom"));
      await call.catch(() => {});
    });
    expect(result.current.submitting).toBe(false);
  });

  it("allows a fresh invocation once the previous one has settled", async () => {
    const d1 = deferred<void>();
    const d2 = deferred<void>();
    const fn = vi.fn().mockReturnValueOnce(d1.promise).mockReturnValueOnce(d2.promise);
    const { result } = renderHook(() => useSubmitGuard());

    await act(async () => {
      void result.current.guard(fn)();
    });
    await act(async () => {
      d1.resolve();
      await d1.promise;
    });
    expect(fn).toHaveBeenCalledTimes(1);

    await act(async () => {
      void result.current.guard(fn)();
    });
    expect(fn).toHaveBeenCalledTimes(2);

    await act(async () => {
      d2.resolve();
      await d2.promise;
    });
  });
});
