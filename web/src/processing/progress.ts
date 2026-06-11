export type ProcessingStage = "decode" | "ingest" | "prepare" | "probe" | "finalize";

export interface ProgressEvent {
  stage: ProcessingStage;
  /** Progress within the current stage, 0..1. */
  fraction: number;
  /** Progress across the whole job, 0..1. */
  overall: number;
}

/** Rough relative cost of each stage; normalized over the stages a job runs. */
export const STAGE_WEIGHTS: Record<ProcessingStage, number> = {
  decode: 0.1,
  ingest: 0.25,
  prepare: 0.2,
  probe: 0.35,
  finalize: 0.1,
};

/**
 * Aggregates per-stage progress into a single 0..1 value. Stages run
 * sequentially: reporting on stage N marks all earlier stages complete, and
 * the overall value never decreases (late events from earlier stages are
 * absorbed).
 */
export class ProgressAggregator {
  private readonly stages: ProcessingStage[];
  private readonly emit: (e: ProgressEvent) => void;
  private readonly total: number;
  private readonly fractions = new Map<ProcessingStage, number>();
  private lastOverall = 0;

  constructor(stages: ProcessingStage[], emit: (e: ProgressEvent) => void) {
    this.stages = stages;
    this.emit = emit;
    this.total = stages.reduce((sum, s) => sum + STAGE_WEIGHTS[s], 0);
  }

  update(stage: ProcessingStage, fraction: number): void {
    const idx = this.stages.indexOf(stage);
    if (idx < 0) return;
    const f = Math.min(1, Math.max(0, fraction));
    for (let i = 0; i < idx; i++) this.fractions.set(this.stages[i], 1);
    this.fractions.set(stage, Math.max(f, this.fractions.get(stage) ?? 0));

    let acc = 0;
    for (const s of this.stages) acc += STAGE_WEIGHTS[s] * (this.fractions.get(s) ?? 0);
    this.lastOverall = Math.max(this.lastOverall, acc / this.total);
    this.emit({ stage, fraction: f, overall: this.lastOverall });
  }

  complete(stage: ProcessingStage): void {
    this.update(stage, 1);
  }
}
