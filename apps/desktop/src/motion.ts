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

/**
 * Springs mirroring `--e-spring` (small elements: badges, chips, popovers) and `--e-soft`
 * (large movements: sidebar, active indicator).
 */
export const spring = {
  pop: { type: 'spring', visualDuration: duration.d3, bounce: 0.35 },
  soft: { type: 'spring', visualDuration: duration.d4, bounce: 0.12 },
} as const;
