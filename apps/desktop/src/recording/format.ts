/** A duration as a clock: `mm:ss` under an hour, then `h:mm:ss`. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const two = (n: number) => String(n).padStart(2, '0');
  return hours > 0 ? `${hours}:${two(minutes)}:${two(seconds)}` : `${two(minutes)}:${two(seconds)}`;
}

/** Quietest level the meters show, in dBFS. */
const FLOOR_DB = -60;

/** A level in dBFS as the filled part of a meter, from 0 to 1. */
export function meterFill(db: number): number {
  if (!Number.isFinite(db)) return 0;
  return Math.min(1, Math.max(0, (db - FLOOR_DB) / -FLOOR_DB));
}

/** A duration in whole minutes, at least 1, for meeting metadata. */
export function minutes(ms: number): number {
  return Math.max(1, Math.round(ms / 60_000));
}
