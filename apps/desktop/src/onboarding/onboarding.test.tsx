import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import enOnboarding from '../../../../locales/en/onboarding.ftl?raw';
import itOnboarding from '../../../../locales/it/onboarding.ftl?raw';
import { App } from '../App';
import { I18nProvider } from '../i18n/I18nProvider';
import type { AppSettings, LibrarySettings, ModelStatus, TaskModel } from '../settings/api';
import type { Permission } from './api';
import { Onboarding } from './Onboarding';
import { setupNeeds } from './SetupTile';
import { nextStep, previousStep, resumeStep, STEPS, type Step } from './steps';

const appSettings: AppSettings = {
  interfaceLanguage: null,
  theme: 'system',
  accent: 'lime',
  reduceTransparency: false,
  reduceMotion: true,
  microphone: null,
  globalShortcut: 'Alt+Super+KeyR',
  defaultGlobalShortcut: 'Alt+Super+KeyR',
  globalShortcutError: null,
  library: {
    current: '/Users/test/Library/Application Support/Trama',
    next: '/Users/test/Library/Application Support/Trama',
    default: '/Users/test/Library/Application Support/Trama',
    overridden: false,
    restartRequired: false,
  },
};

const librarySettings: LibrarySettings = {
  meetingLanguage: 'en',
  audioRetention: 'never',
  currency: 'USD',
  monthlyBudgetMicros: null,
  warnPercent: 80,
  enforceBudget: false,
};

const missingModel: ModelStatus = {
  state: 'missing',
  downloadedBytes: 0,
  totalBytes: 2_500_000_000,
  errorCode: null,
  progress: null,
};

interface Shell {
  microphone?: Permission;
  microphoneAnswer?: Permission;
  systemAudio?: Permission;
  model?: ModelStatus;
  tasks?: TaskModel[];
}

type Calls = { cmd: string; args: Record<string, unknown> }[];

/** Mocks the commands the onboarding uses. Permissions are fakes: nothing asks macOS. */
function mockShell(shell: Shell = {}) {
  const calls: Calls = [];
  let microphone = shell.microphone ?? 'not_determined';
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    switch (cmd) {
      case 'app_settings':
        return appSettings;
      case 'library_settings':
        return librarySettings;
      case 'set_library_settings':
        return args.settings;
      case 'microphone_permission':
        return microphone;
      case 'request_microphone_permission':
        microphone = shell.microphoneAnswer ?? 'granted';
        return microphone;
      case 'check_system_audio_permission':
        return shell.systemAudio ?? 'granted';
      case 'list_providers':
        return [];
      case 'task_models':
        return shell.tasks ?? [];
      case 'model_status':
        return shell.model ?? missingModel;
      case 'open_at_login':
        return false;
      case 'plugin:event|listen':
        return 1;
      default:
        return null;
    }
  });
  return calls;
}

const steps = (calls: Calls) =>
  calls.filter((call) => call.cmd === 'set_onboarding_step').map((call) => call.args.step);

function renderOnboarding(initialStep: Step = 'welcome', locale: 'en' | 'it' = 'en') {
  const user = userEvent.setup();
  const onFinish = vi.fn();
  render(
    <I18nProvider locale={locale}>
      <Onboarding initialStep={initialStep} onFinish={onFinish} />
    </I18nProvider>,
  );
  return { user, onFinish };
}

const heading = (name: string | RegExp) => screen.findByRole('heading', { level: 1, name });

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

describe('steps', () => {
  it('resumes at a known step and restarts on an unknown one', () => {
    expect(resumeStep('model')).toBe('model');
    expect(resumeStep('done')).toBe('welcome');
    expect(resumeStep('from_a_newer_version')).toBe('welcome');
    expect(resumeStep(null)).toBe('welcome');
  });

  it('moves one step at a time and stays at the ends', () => {
    expect(nextStep('welcome')).toBe('language');
    expect(nextStep('ready')).toBe('ready');
    expect(previousStep('welcome')).toBe('welcome');
    expect(previousStep('ready')).toBe('everyday');
    expect(STEPS[0]).toBe('welcome');
    expect(STEPS.at(-1)).toBe('ready');
  });
});

describe('setupNeeds', () => {
  it('waits until everything has been read', () => {
    expect(setupNeeds(undefined, true, 'granted')).toBeNull();
    expect(setupNeeds('ready', null, 'granted')).toBeNull();
    expect(setupNeeds('ready', true, null)).toBeNull();
  });

  it('lists what recording a meeting still needs', () => {
    expect(setupNeeds('ready', true, 'granted')).toEqual([]);
    expect(setupNeeds('missing', false, 'denied')).toEqual(['microphone', 'model', 'provider']);
    // A download in progress is on its way; an unreadable permission is not reported.
    expect(setupNeeds('downloading', true, 'unknown')).toEqual([]);
    expect(setupNeeds('corrupt', true, 'not_determined')).toEqual(['microphone', 'model']);
  });
});

describe('Onboarding', () => {
  it('walks through the steps, storing each one, and finishes', async () => {
    const calls = mockShell({ model: { ...missingModel, state: 'ready' } });
    const { user, onFinish } = renderOnboarding();
    expect(await heading('Welcome to Trama')).toBeInTheDocument();
    expect(screen.getByText('Step 1 of 7')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Get started' }));
    expect(await heading('What language are your meetings in?')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await heading('Let Trama hear the meeting')).toBeInTheDocument();
    // Nothing set up yet: the step can wait.
    await user.click(screen.getByRole('button', { name: 'Not now' }));
    expect(await heading('Choose who writes the summaries')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Not now' }));
    // The model is already there: nothing to postpone.
    expect(await heading('Download the transcription model')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await heading('Always at hand, in the menu bar')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Continue' }));
    expect(await heading('Ready, with a few things for later')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Start using Trama' }));

    expect(onFinish).toHaveBeenCalledOnce();
    expect(steps(calls)).toEqual([...STEPS.slice(1), 'done']);
  });

  it('can be skipped from any step', async () => {
    const calls = mockShell();
    const { user, onFinish } = renderOnboarding('provider');
    expect(await heading('Choose who writes the summaries')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Skip setup' }));
    expect(onFinish).toHaveBeenCalledOnce();
    expect(steps(calls)).toEqual(['done']);
  });

  it('jumps to any step from the list and marks the ones done', async () => {
    mockShell({ tasks: [{ task: 'summary', providerId: 'p', modelId: 'm' }] });
    const { user } = renderOnboarding();
    const list = await screen.findByRole('navigation', { name: 'Setup steps' });
    await user.click(within(list).getByRole('button', { name: /Transcription/ }));
    expect(await heading('Download the transcription model')).toBeInTheDocument();
    expect(within(list).getByRole('button', { name: /Transcription/ })).toHaveAttribute(
      'aria-current',
      'step',
    );
    // A model is chosen for summaries: that step is done even if never opened.
    await waitFor(() =>
      expect(within(list).getByRole('button', { name: /Summaries/ })).toHaveAttribute('data-done'),
    );
  });

  it('asks for the microphone and checks system audio only when the user clicks', async () => {
    const calls = mockShell();
    const { user } = renderOnboarding('permissions');
    await heading('Let Trama hear the meeting');
    const mic = await screen.findByRole('article', { name: 'Microphone' });
    const system = screen.getByRole('article', { name: 'System audio' });
    expect(calls.some((call) => call.cmd === 'request_microphone_permission')).toBe(false);
    expect(calls.some((call) => call.cmd === 'check_system_audio_permission')).toBe(false);

    await user.click(within(mic).getByRole('button', { name: 'Allow microphone' }));
    expect(await within(mic).findByText('Allowed')).toBeInTheDocument();
    await user.click(within(system).getByRole('button', { name: 'Allow system audio' }));
    expect(await within(system).findByText('Allowed')).toBeInTheDocument();
    // Both allowed: nothing left to postpone.
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument();
  });

  it('explains a refusal and opens the right pane of System Settings', async () => {
    const calls = mockShell({ microphone: 'denied', systemAudio: 'denied' });
    const { user } = renderOnboarding('permissions');
    const mic = await screen.findByRole('article', { name: 'Microphone' });
    expect(await within(mic).findByText(/Microphone access is off/)).toBeInTheDocument();
    await user.click(within(mic).getByRole('button', { name: 'Open System Settings' }));

    const system = screen.getByRole('article', { name: 'System audio' });
    await user.click(within(system).getByRole('button', { name: 'Allow system audio' }));
    expect(await within(system).findByText(/couldn't hear the system audio/)).toBeInTheDocument();
    await user.click(within(system).getByRole('button', { name: 'Open System Settings' }));

    expect(
      calls.filter((call) => call.cmd === 'open_privacy_settings').map((call) => call.args.pane),
    ).toEqual(['microphone', 'system_audio']);
  });

  it('sums up what was left for later, with a way back to it', async () => {
    mockShell({ microphone: 'granted' });
    const { user } = renderOnboarding('ready');
    const checks = await screen.findByRole('list', { name: 'Setup' });
    const row = (label: string) =>
      within(checks)
        .getAllByRole('listitem')
        .find((item) => item.textContent?.includes(label));
    await waitFor(() => expect(row('Microphone')).toHaveTextContent('Allowed'));
    expect(row('System audio')).toHaveTextContent('Not checked');
    expect(row('Transcription model')).toHaveTextContent('Not downloaded');
    expect(row('Recording shortcut')).toHaveTextContent('⌥⌘R');
    // Notes kept elsewhere can come in through the Markdown import.
    expect(screen.getByText(/Import Notes… \(⌘O\)/)).toBeInTheDocument();

    await user.click(within(row('Transcription model')!).getByRole('button', { name: 'Set up' }));
    expect(await heading('Download the transcription model')).toBeInTheDocument();
  });

  it('saves the meeting language', async () => {
    const calls = mockShell();
    const { user } = renderOnboarding('language', 'it');
    expect(await heading('In che lingua sono i tuoi meeting?')).toBeInTheDocument();
    await user.click(await screen.findByRole('button', { name: /Inglese/ }));
    await user.click(await screen.findByRole('option', { name: 'Tedesco' }));
    await waitFor(() =>
      expect(calls.find((call) => call.cmd === 'set_library_settings')?.args.settings).toEqual({
        ...librarySettings,
        meetingLanguage: 'de',
      }),
    );
    // Inside the sentence the language is written as in running text.
    expect(await screen.findByText(/trascritti in tedesco/)).toBeInTheDocument();
  });
});

describe('App and the onboarding', () => {
  it('starts with the onboarding when it is pending, and the shell after it', async () => {
    mockShell();
    const user = userEvent.setup();
    render(
      <I18nProvider locale="en">
        <App onboarding="model" />
      </I18nProvider>,
    );
    expect(await heading('Download the transcription model')).toBeInTheDocument();
    expect(screen.queryByRole('complementary', { name: 'Sidebar' })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Skip setup' }));
    expect(await screen.findByRole('complementary', { name: 'Sidebar' })).toBeInTheDocument();
  });

  it('reopens the setup from the command bar', async () => {
    mockShell();
    const user = userEvent.setup();
    render(
      <I18nProvider locale="en">
        <App />
      </I18nProvider>,
    );
    await user.click(screen.getByRole('button', { name: 'Commands' }));
    await user.click(await screen.findByRole('menuitem', { name: /Set up Trama/ }));
    expect(await heading('Welcome to Trama')).toBeInTheDocument();
  });
});

describe('catalogs', () => {
  const ids = (source: string) =>
    [...source.matchAll(/^([a-z][a-z0-9-]*) =/gm)].map((match) => match[1]).sort();

  it('has every onboarding message in English and Italian', () => {
    expect(ids(itOnboarding)).toEqual(ids(enOnboarding));
    expect(ids(enOnboarding).length).toBeGreaterThan(50);
  });
});
