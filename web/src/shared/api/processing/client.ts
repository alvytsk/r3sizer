import type { AutoSharpParams, ProcessResult } from "@/shared/lib";
import { CancellationToken } from "./errors";
import { bitmapToRgba, decodeToBitmap, extractStripes, makePreview, planStripes } from "./ingest";
import { destroyProbePool } from "./probe-pool";
import { type ProcessingStage, ProgressAggregator, type ProgressEvent } from "./progress";
import {
  clearAllCaches,
  ingestAbortFireAndForget,
  ingestBegin,
  ingestEnd,
  ingestStripe,
  prepareBaseImage,
  prepareImage,
  processImageParallel,
  resetWorker,
  setProgressCallback,
} from "./wasm";

/**
 * Images above this pixel count take the striped ingest path. This is a web
 * product policy, not a core policy: core provides the building blocks, the
 * app decides when to use them.
 */
export const DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS = 24_000_000;

export interface DecodedInput {
  /** Original source dimensions. */
  width: number;
  height: number;
  striped: boolean;
  /** Displayable pixels: full-size for monolithic, downscaled for striped. */
  preview: { rgbaData: Uint8Array; width: number; height: number };
}

export interface ProcessJob {
  readonly promise: Promise<ProcessResult>;
  onProgress(cb: (e: ProgressEvent) => void): void;
  cancel(): void;
}

interface ClientInput {
  file: File;
  bitmap: ImageBitmap;
  striped: boolean;
  /** Full-size RGBA — materialized only on the monolithic path. */
  rgba: { data: Uint8Array; width: number; height: number } | null;
}

export class ProcessingClient {
  private input: ClientInput | null = null;
  private activeJob: JobImpl | null = null;

  /**
   * Decode a file and cache it for subsequent process() calls. Decode
   * failures (e.g. Safari refusing a giant JPEG) reject here — that is the
   * spec's decode-stage error.
   */
  async decode(file: File): Promise<DecodedInput> {
    this.input?.bitmap.close();
    this.input = null;

    const bitmap = await decodeToBitmap(file);
    const striped = bitmap.width * bitmap.height > DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS;
    const rgba = striped ? null : bitmapToRgba(bitmap);
    this.input = { file, bitmap, striped, rgba };

    // New image: stale caches must not be reused.
    void clearAllCaches().catch(() => {});
    // Warm the linear-conversion cache (monolithic only, same as before).
    if (rgba) void prepareImage(rgba.data, rgba.width, rgba.height).catch(() => {});

    const preview = striped
      ? makePreview(bitmap)
      : { rgbaData: rgba!.data, width: rgba!.width, height: rgba!.height };
    return { width: bitmap.width, height: bitmap.height, striped, preview };
  }

  /**
   * Eagerly pre-compute the base while the user reviews params. No-op on the
   * striped path (ingest happens inside the first process() job).
   */
  prewarmBase(params: AutoSharpParams): void {
    const inp = this.input;
    if (!inp?.rgba) return;
    void prepareBaseImage(
      inp.rgba.data,
      inp.rgba.width,
      inp.rgba.height,
      JSON.stringify(params),
    ).catch(() => {});
  }

  /** Start a processing job. One active job at a time: a new job cancels the old. */
  process(params: AutoSharpParams): ProcessJob {
    if (!this.input) throw new Error("No input decoded — call decode(file) first");
    this.activeJob?.cancel();
    const job = new JobImpl(this.input, params);
    this.activeJob = job;
    void job.promise
      .catch(() => {})
      .finally(() => {
        if (this.activeJob === job) this.activeJob = null;
      });
    return job;
  }

  /**
   * Emergency recovery for a hung worker: terminate workers and probe pool;
   * the next call re-initializes WASM from scratch (cache loss).
   */
  async reset(): Promise<void> {
    this.activeJob?.cancel();
    this.activeJob = null;
    destroyProbePool();
    resetWorker();
  }
}

class JobImpl implements ProcessJob {
  readonly promise: Promise<ProcessResult>;
  private readonly token = new CancellationToken();
  private readonly listeners: Array<(e: ProgressEvent) => void> = [];
  private readonly aggregator: ProgressAggregator;

  constructor(input: ClientInput, params: AutoSharpParams) {
    const stages: ProcessingStage[] = input.striped
      ? ["ingest", "prepare", "probe", "finalize"]
      : ["prepare", "probe", "finalize"];
    this.aggregator = new ProgressAggregator(stages, (e) => {
      for (const l of this.listeners) l(e);
    });
    this.promise = this.run(input, params);
  }

  onProgress(cb: (e: ProgressEvent) => void): void {
    this.listeners.push(cb);
  }

  cancel(): void {
    this.token.cancel();
  }

  private async run(input: ClientInput, params: AutoSharpParams): Promise<ProcessResult> {
    // Map the worker's legacy progress strings onto coarse stage events.
    setProgressCallback((stage) => {
      if (stage === "probing") this.aggregator.update("probe", 0);
      else if (stage === "fitting" || stage === "encoding") this.aggregator.update("finalize", 0.5);
      else this.aggregator.update("prepare", 0.5);
    });
    try {
      const result = input.striped
        ? await this.runStriped(input, params)
        : await this.runMonolithic(input, params);
      this.aggregator.complete("finalize");
      return result;
    } finally {
      setProgressCallback(null);
    }
  }

  private async runMonolithic(input: ClientInput, params: AutoSharpParams): Promise<ProcessResult> {
    this.token.throwIfCancelled();
    this.aggregator.update("prepare", 0);
    const { data, width, height } = input.rgba!;
    const result = await processImageParallel(data, width, height, JSON.stringify(params), {
      token: this.token,
      onProbeProgress: (f) => this.aggregator.update("probe", f),
    });
    this.token.throwIfCancelled();
    return result;
  }

  private async runStriped(input: ClientInput, params: AutoSharpParams): Promise<ProcessResult> {
    // Content-adaptive resize and full source-side diagnostics need the
    // whole source image, which never exists on this path.
    const effectiveParams: AutoSharpParams = {
      ...params,
      resize_strategy: null,
      diagnostics_level: "summary",
    };

    const { bitmap } = input;
    this.aggregator.update("ingest", 0);
    let inter: { width: number; height: number };
    try {
      await ingestBegin(bitmap.width, bitmap.height, params.target_width, params.target_height);
      const { count } = planStripes(bitmap.width, bitmap.height);
      let sent = 0;
      for await (const { rgba, rows } of extractStripes(bitmap)) {
        this.token.throwIfCancelled();
        await ingestStripe(rgba, rows);
        sent++;
        this.aggregator.update("ingest", sent / count);
      }
      this.token.throwIfCancelled();
      inter = await ingestEnd();
    } catch (err) {
      ingestAbortFireAndForget();
      throw err;
    }

    this.aggregator.update("prepare", 0);
    // Empty pixel data: the intermediate is already the worker's cached input.
    const result = await processImageParallel(
      new Uint8Array(0),
      inter.width,
      inter.height,
      JSON.stringify(effectiveParams),
      { token: this.token, onProbeProgress: (f) => this.aggregator.update("probe", f) },
    );
    this.token.throwIfCancelled();
    return result;
  }
}

export const processingClient = new ProcessingClient();
