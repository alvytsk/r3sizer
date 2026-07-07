import { useCallback, useEffect, useRef } from "react";
import { useImageStore } from "@/entities/images";
import type { AutoSharpParams } from "@/shared/lib";

type AnyFn = (...args: never[]) => void;

/**
 * Leading + trailing throttle: the first call fires immediately, subsequent
 * calls are collapsed into a single trailing invocation `wait` ms later.
 * The returned function is stable while `fn` is stable (e.g. a zustand action).
 */
function useThrottledCallback<T extends AnyFn>(fn: T, wait = 80): T {
  const lastRun = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingArgs = useRef<Parameters<T> | null>(null);

  useEffect(() => {
    return () => {
      if (timer.current !== null) clearTimeout(timer.current);
    };
  }, []);

  return useCallback(
    (...args: Parameters<T>) => {
      const now = Date.now();
      const remaining = wait - (now - lastRun.current);
      pendingArgs.current = args;

      if (remaining <= 0) {
        if (timer.current !== null) {
          clearTimeout(timer.current);
          timer.current = null;
        }
        lastRun.current = now;
        fn(...args);
        pendingArgs.current = null;
      } else if (timer.current === null) {
        timer.current = setTimeout(() => {
          lastRun.current = Date.now();
          timer.current = null;
          if (pendingArgs.current) {
            const p = pendingArgs.current;
            pendingArgs.current = null;
            fn(...p);
          }
        }, remaining);
      }
    },
    [fn, wait],
  ) as T;
}

/**
 * Trailing debounce: fires once the caller has been quiet for `wait` ms.
 * The returned function is stable while `fn` is stable (e.g. a zustand action).
 */
function useDebouncedCallback<T extends AnyFn>(fn: T, wait = 300): T {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timer.current !== null) clearTimeout(timer.current);
    };
  }, []);

  return useCallback(
    (...args: Parameters<T>) => {
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = setTimeout(() => {
        timer.current = null;
        fn(...args);
      }, wait);
    },
    [fn, wait],
  ) as T;
}

/**
 * Throttled wrapper around `updateParams` for continuous inputs (sliders).
 * Fires at most once per `wait` ms during drag, giving progressive visual
 * feedback without flooding the store with ~60 updates/sec.
 */
export function useThrottledUpdateParams(wait = 80) {
  const updateParams = useImageStore((s) => s.updateParams);
  return useThrottledCallback((partial: Partial<AutoSharpParams>) => updateParams(partial), wait);
}

/**
 * Debounced wrapper around `updateParams` for text inputs.
 * Waits until the user stops typing for `wait` ms before committing.
 */
export function useDebouncedUpdateParams(wait = 300) {
  const updateParams = useImageStore((s) => s.updateParams);
  return useDebouncedCallback((partial: Partial<AutoSharpParams>) => updateParams(partial), wait);
}
