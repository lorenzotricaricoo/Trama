import { invoke } from '@tauri-apps/api/core';
import type { Accent, Theme } from '../appearance';

/**
 * The settings commands of the desktop shell (see `src-tauri/src/settings.rs` and
 * `src-tauri/src/providers.rs`). Every failure rejects with an error code to translate.
 */

export interface LibraryLocation {
  current: string;
  next: string;
  default: string;
  overridden: boolean;
  restartRequired: boolean;
}

export interface AppSettings {
  interfaceLanguage: string | null;
  theme: Theme;
  accent: Accent;
  reduceTransparency: boolean;
  reduceMotion: boolean;
  microphone: string | null;
  globalShortcut: string;
  defaultGlobalShortcut: string;
  library: LibraryLocation;
}

export const RETENTIONS = ['never', '7_days', '30_days', 'always'] as const;
export type Retention = (typeof RETENTIONS)[number];

export interface LibrarySettings {
  meetingLanguage: string;
  audioRetention: Retention;
  currency: string;
  /** Millionths of the currency unit. */
  monthlyBudgetMicros: number | null;
  warnPercent: number;
  enforceBudget: boolean;
}

/** What a retention setting would delete of the audio that exists now. */
export interface RetentionImpact {
  /** Recordings whose audio would be deleted right away. */
  deleteNow: number;
  /** Recordings whose audio would be deleted when their period ends. */
  deleteLater: number;
  /** Days the setting keeps audio after the transcription; `null` for "always". */
  days: number | null;
}

/** Audio recorded before audio retention existed, kept until the user confirms. */
export interface HeldAudio {
  count: number;
  /** What the current setting would delete of it. */
  impact: RetentionImpact;
}

export interface Budget {
  periodStart: number;
  requests: number;
  inputTokens: number;
  outputTokens: number;
  spentMicros: number;
  unpricedRequests: number;
  budgetMicros: number | null;
  warnPercent: number;
  enforce: boolean;
  state: 'no_budget' | 'ok' | 'warning' | 'exceeded';
}

export interface ProviderModel {
  modelId: string;
  listed: boolean;
  catalogContextTokens: number | null;
  catalogMaxOutputTokens: number | null;
  contextTokensOverride: number | null;
  maxOutputTokensOverride: number | null;
  /** Per million tokens, in millionths of the currency unit. */
  inputPriceMicros: number | null;
  outputPriceMicros: number | null;
}

export interface Provider {
  id: string;
  name: string;
  baseUrl: string;
  needsKey: boolean;
  hasKey: boolean;
  catalogFetchedAt: number | null;
  models: ProviderModel[];
}

export interface ProviderInput {
  name: string;
  baseUrl: string;
  needsKey: boolean;
  /** Only when the user typed one; never read back. */
  apiKey?: string;
}

export interface ModelSettings {
  providerId: string;
  modelId: string;
  contextTokens: number | null;
  maxOutputTokens: number | null;
  inputPriceMicros: number | null;
  outputPriceMicros: number | null;
}

/** Tasks that need a language model; phase 1 has the summary. */
export const TASKS = ['summary'] as const;
export type Task = (typeof TASKS)[number];

export interface TaskModel {
  task: Task;
  providerId: string;
  modelId: string;
}

export interface InputDevices {
  devices: { id: string; name: string }[];
  defaultId: string | null;
}

export interface ChosenFolder {
  path: string;
  warning: string | null;
  error: string | null;
}

export interface ModelStatus {
  state: 'missing' | 'downloading' | 'verifying' | 'ready' | 'corrupt';
  downloadedBytes: number;
  totalBytes: number;
  errorCode: string | null;
  progress: number | null;
}

/** A problem found at start; Trama started anyway. */
export interface StartupNotice {
  code: 'settings_reset' | 'settings_unreadable' | 'library_unavailable' | 'library_failed';
  path: string | null;
}

/** Event the core sends for every change of the transcription model. */
export const MODEL_EVENT = 'model';

/** An error code from a rejected command; anything else is reported and becomes `internal`. */
export function errorCode(error: unknown): string {
  if (typeof error === 'string' && /^[a-z0-9_]+$/.test(error)) return error;
  console.error('Settings command failed', error);
  return 'internal';
}

export const api = {
  appSettings: () => invoke<AppSettings>('app_settings'),
  setInterfaceLanguage: (language: string | null) =>
    invoke<string>('set_interface_language', { language }),
  setAppearance: (theme: Theme, accent: Accent) =>
    invoke<void>('set_appearance', { theme, accent }),
  setReduceTransparency: (enabled: boolean) => invoke<void>('set_reduce_transparency', { enabled }),
  setReduceMotion: (enabled: boolean) => invoke<void>('set_reduce_motion', { enabled }),
  setMicrophone: (device: string | null) => invoke<void>('set_microphone', { device }),
  setGlobalShortcut: (shortcut: string) => invoke<void>('set_global_shortcut', { shortcut }),
  inputDevices: () => invoke<InputDevices>('input_devices'),
  chooseLibraryFolder: () => invoke<ChosenFolder | null>('choose_library_folder'),
  setLibraryLocation: (path: string | null) =>
    invoke<LibraryLocation>('set_library_location', { path }),
  revealLibrary: () => invoke<void>('reveal_library'),
  startupNotices: () => invoke<StartupNotice[]>('startup_notices'),
  librarySettings: () => invoke<LibrarySettings>('library_settings'),
  /** `confirmAudioDeletion`: the user confirmed that existing audio may be deleted. */
  setLibrarySettings: (settings: LibrarySettings, confirmAudioDeletion = false) =>
    invoke<LibrarySettings>(
      'set_library_settings',
      confirmAudioDeletion ? { settings, confirmAudioDeletion } : { settings },
    ),
  audioRetentionImpact: (retention: Retention) =>
    invoke<RetentionImpact>('audio_retention_impact', { retention }),
  heldAudio: () => invoke<HeldAudio>('held_audio'),
  applyRetentionToHeldAudio: () => invoke<HeldAudio>('apply_retention_to_held_audio'),
  budgetStatus: () => invoke<Budget>('budget_status'),
  listProviders: () => invoke<Provider[]>('list_providers'),
  addProvider: (input: ProviderInput) => invoke<Provider>('add_provider', { input }),
  updateProvider: (id: string, input: ProviderInput) =>
    invoke<Provider>('update_provider', { id, input }),
  removeProvider: (id: string) => invoke<void>('remove_provider', { id }),
  testProvider: (id: string) => invoke<Provider>('test_provider', { id }),
  setModelSettings: (input: ModelSettings) =>
    invoke<ProviderModel>('set_model_settings', { input }),
  taskModels: () => invoke<TaskModel[]>('task_models'),
  setTaskModel: (task: Task, choice: Omit<TaskModel, 'task'> | null) =>
    invoke<void>('set_task_model', { task, choice: choice && { task, ...choice } }),
  modelStatus: () => invoke<ModelStatus>('model_status'),
  downloadModel: () => invoke<ModelStatus>('download_model'),
  cancelModelDownload: () => invoke<ModelStatus>('cancel_model_download'),
  verifyModel: () => invoke<ModelStatus>('verify_model'),
  removeModel: () => invoke<ModelStatus>('remove_model'),
};

/** Millionths of a currency unit to the unit, and back. */
export const fromMicros = (micros: number | null) => (micros === null ? null : micros / 1e6);
export const toMicros = (value: number | null) =>
  value === null || Number.isNaN(value) ? null : Math.round(value * 1e6);
