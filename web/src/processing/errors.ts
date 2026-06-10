import type { ProcessingStage } from "./progress";

/** The job was cancelled by the user. Not an error condition for the UI. */
export class CancelledError extends Error {
  constructor() {
    super("Processing cancelled");
    this.name = "CancelledError";
  }
}

/** A stage failed; `stage` tells the UI where. */
export class ProcessingError extends Error {
  readonly stage: ProcessingStage;

  constructor(stage: ProcessingStage, message: string) {
    super(message);
    this.name = "ProcessingError";
    this.stage = stage;
  }
}

/** Cooperative cancellation flag, checked between WASM calls. */
export class CancellationToken {
  private flag = false;

  cancel(): void {
    this.flag = true;
  }

  get cancelled(): boolean {
    return this.flag;
  }

  throwIfCancelled(): void {
    if (this.flag) throw new CancelledError();
  }
}
