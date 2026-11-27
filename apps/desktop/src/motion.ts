/**
 * Motion values for the Motion library, mirroring the motion tokens in
 * docs/design/tokens.css (durations in seconds, easings as cubic-bezier points).
 */
export const duration = {
  d1: 0.12,
  d2: 0.2,
  d3: 0.32,
  d4: 0.52,
} as const;

export const ease = {
  out: [0.22, 1, 0.36, 1],
  in: [0.55, 0, 0.8, 0.3],
  inOut: [0.65, 0, 0.35, 1],
} as const;
