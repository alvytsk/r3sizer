import init, { process_image } from "./wasm-pkg/imgsharp_wasm";

let initialized = false;

export async function initWasm(): Promise<void> {
  if (!initialized) {
    await init();
    initialized = true;
  }
}

export { process_image };
