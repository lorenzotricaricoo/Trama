import { invoke } from '@tauri-apps/api/core';

/**
 * The onboarding commands of the desktop shell (see `src-tauri/src/onboarding.rs`). Every
 * failure rejects with an error code to translate.
 */

/** A permission as macOS reports it; `unknown` when it can't be read (system audio). */
export type Permission = 'granted' | 'denied' | 'restricted' | 'not_determined' | 'unknown';

export interface OnboardingState {
  /** The step to show at start; `null` when the onboarding is done. */
  pending: string | null;
}

/** Panes of System Settings › Privacy & Security. */
export type PrivacyPane = 'microphone' | 'system_audio';

export const onboardingApi = {
  state: () => invoke<OnboardingState>('onboarding_state'),
  /** A step name, or `done`. */
  setStep: (step: string) => invoke<void>('set_onboarding_step', { step }),
  microphone: () => invoke<Permission>('microphone_permission'),
  /** Shows the macOS prompt if it was never shown; resolves with the answer. */
  requestMicrophone: () => invoke<Permission>('request_microphone_permission'),
  /** Plays an inaudible probe (about a second); the first time macOS asks the user. */
  checkSystemAudio: () => invoke<Permission>('check_system_audio_permission'),
  openPrivacySettings: (pane: PrivacyPane) => invoke<void>('open_privacy_settings', { pane }),
};
