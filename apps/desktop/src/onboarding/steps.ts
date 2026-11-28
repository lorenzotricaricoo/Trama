import type { Icon } from '../ui/icons';
import {
  AppWindowIcon,
  CheckCircleIcon,
  HandWavingIcon,
  MicrophoneIcon,
  SparkleIcon,
  TranslateIcon,
  WaveformIcon,
} from '../ui/icons';

/** The steps of flow F1, in order. Every one can be left for later. */
export const STEPS = [
  'welcome',
  'language',
  'permissions',
  'provider',
  'model',
  'everyday',
  'ready',
] as const;
export type Step = (typeof STEPS)[number];

/** Stored when the onboarding is finished or skipped (`ONBOARDING_DONE` in the core). */
export const DONE = 'done';

export const STEP_ICONS: Record<Step, Icon> = {
  welcome: HandWavingIcon,
  language: TranslateIcon,
  permissions: MicrophoneIcon,
  provider: SparkleIcon,
  model: WaveformIcon,
  everyday: AppWindowIcon,
  ready: CheckCircleIcon,
};

export function isStep(value: string | null | undefined): value is Step {
  return (STEPS as readonly (string | null | undefined)[]).includes(value);
}

/** A stored step this version knows, or the first one: a name from another version restarts. */
export function resumeStep(stored: string | null): Step {
  return isStep(stored) ? stored : STEPS[0];
}

export function stepIndex(step: Step): number {
  return STEPS.indexOf(step);
}

/** The step after `step`; the last one stays. */
export function nextStep(step: Step): Step {
  return STEPS[Math.min(stepIndex(step) + 1, STEPS.length - 1)] ?? step;
}

/** The step before `step`; the first one stays. */
export function previousStep(step: Step): Step {
  return STEPS[Math.max(stepIndex(step) - 1, 0)] ?? step;
}
