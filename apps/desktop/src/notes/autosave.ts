/** Where an autosaver stands, for the status shown next to the editor. */
export type SaveStatus = 'idle' | 'pending' | 'saving' | 'saved' | 'error';

export interface AutosaveTiming {
  /** Pause in the typing after which the text is saved, in ms. */
  delay: number;
  /** Longest time an edit waits while the typing goes on, in ms. */
  maxWait: number;
  /** Wait before trying again after a failed save, in ms. */
  retry: number;
}

/**
 * Short enough that a forced quit loses at most a fraction of a second of typing; each
 * save is one small SQLite transaction.
 */
export const AUTOSAVE_TIMING: AutosaveTiming = { delay: 300, maxWait: 1500, retry: 3000 };

/**
 * Saves a text some time after it changes: after a pause in the typing, at least every
 * `maxWait` while it goes on, and at once on {@link Autosaver.flush}. Saves run one at a
 * time and only the latest text is written; a failed save is tried again.
 */
export class Autosaver {
  /** Text not written yet; null when everything is saved or being saved. */
  private unsaved: string | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  /** When the oldest unsaved edit was made. */
  private since: number | undefined;
  private running: Promise<boolean> | undefined;
  /** Waiting for {@link Autosaver.whenClean}. */
  private cleanWaiters: (() => void)[] = [];
  private readonly save: (text: string) => Promise<unknown>;
  private readonly onStatus: (status: SaveStatus) => void;
  private readonly timing: AutosaveTiming;

  constructor(
    save: (text: string) => Promise<unknown>,
    onStatus: (status: SaveStatus) => void = () => {},
    timing: AutosaveTiming = AUTOSAVE_TIMING,
  ) {
    this.save = save;
    this.onStatus = onStatus;
    this.timing = timing;
  }

  /** Whether some text is not written yet. */
  get dirty(): boolean {
    return this.unsaved !== null || this.running !== undefined;
  }

  /** Records a new text and schedules its save. */
  change(text: string) {
    this.unsaved = text;
    const now = Date.now();
    this.since ??= now;
    this.onStatus('pending');
    const wait = Math.min(this.timing.delay, this.since + this.timing.maxWait - now);
    this.schedule(Math.max(0, wait));
  }

  /**
   * Saves what is pending now. Resolves to true once everything typed so far is written,
   * or to false if a save failed (the text is kept and tried again later).
   */
  async flush(): Promise<boolean> {
    this.clearTimer();
    for (;;) {
      if (!(await this.run())) return false;
      if (this.unsaved === null) return true;
    }
  }

  /** Resolves once everything typed is written, however many retries it takes. */
  whenClean(): Promise<void> {
    if (!this.dirty) return Promise.resolve();
    return new Promise((resolve) => this.cleanWaiters.push(resolve));
  }

  private schedule(wait: number) {
    this.clearTimer();
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.run();
    }, wait);
  }

  private clearTimer() {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  /** Writes what is pending; false if the write failed. */
  private async run(): Promise<boolean> {
    // One save at a time: wait for the one in progress, then write what is left.
    while (this.running) await this.running;
    if (this.unsaved === null) return true;
    const text = this.unsaved;
    this.unsaved = null;
    this.since = undefined;
    this.onStatus('saving');
    this.running = this.write(text);
    try {
      return await this.running;
    } finally {
      this.running = undefined;
      if (!this.dirty) {
        const waiters = this.cleanWaiters;
        this.cleanWaiters = [];
        for (const resolve of waiters) resolve();
      }
    }
  }

  private async write(text: string): Promise<boolean> {
    try {
      await this.save(text);
      if (this.unsaved === null) this.onStatus('saved');
      return true;
    } catch (error: unknown) {
      console.error('Cannot save the note', error);
      // Keep the text, unless newer text replaced it, and try again later.
      this.unsaved ??= text;
      this.since ??= Date.now();
      this.onStatus('error');
      this.schedule(this.timing.retry);
      return false;
    }
  }
}
