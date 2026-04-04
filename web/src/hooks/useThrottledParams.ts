import { useThrottledCallback, useDebouncedCallback } from "@tanstack/react-pacer";
import { useProcessorStore } from "@/stores/processor-store";
import type { AutoSharpParams } from "@/types/wasm-types";

/**
 * Throttled wrapper around `updateParams` for continuous inputs (sliders).
 * Fires at most once per `wait` ms during drag, giving progressive visual
 * feedback without flooding the store with ~60 updates/sec.
 */
export function useThrottledUpdateParams(wait = 80) {
  const updateParams = useProcessorStore((s) => s.updateParams);
  return useThrottledCallback(
    (partial: Partial<AutoSharpParams>) => updateParams(partial),
    { wait },
  );
}

/**
 * Debounced wrapper around `updateParams` for text inputs.
 * Waits until the user stops typing for `wait` ms before committing.
 */
export function useDebouncedUpdateParams(wait = 300) {
  const updateParams = useProcessorStore((s) => s.updateParams);
  return useDebouncedCallback(
    (partial: Partial<AutoSharpParams>) => updateParams(partial),
    { wait },
  );
}
