import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { createBundle, createTranslate, SUPPORTED_LOCALES } from '../i18n/catalogs';
import { I18nProvider } from '../i18n/I18nProvider';
import type { AppSettings, Budget, LibrarySettings, ModelStatus, Provider } from './api';
import { formatShortcut, shortcutFromEvent } from './RecordingSection';
import { SettingsScreen } from './SettingsScreen';

// A made-up key: tests never talk to a real provider.
const FAKE_KEY = 'sk-test-not-a-real-key';

const appSettings: AppSettings = {
  interfaceLanguage: null,
  theme: 'system',
  accent: 'lime',
  reduceTransparency: false,
  reduceMotion: false,
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

const budget: Budget = {
  periodStart: 0,
  requests: 0,
  inputTokens: 0,
  outputTokens: 0,
  spentMicros: 0,
  unpricedRequests: 0,
  budgetMicros: null,
  warnPercent: 80,
  enforce: false,
  state: 'no_budget',
};

const modelStatus: ModelStatus = {
  state: 'missing',
  downloadedBytes: 0,
  totalBytes: 2_500_000_000,
  errorCode: null,
  progress: null,
};

type Calls = { cmd: string; args: Record<string, unknown> }[];

/** Mocks the shell's commands; `overrides` answers (or rejects) some of them. */
function mockShell(overrides: Record<string, (args: Record<string, unknown>) => unknown> = {}) {
  const calls: Calls = [];
  const providers: Provider[] = [];
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    const override = overrides[cmd];
    if (override) return override(args);
    switch (cmd) {
      case 'app_settings':
        return appSettings;
      case 'library_settings':
        return librarySettings;
      case 'set_library_settings':
        return args.settings;
      case 'audio_retention_impact':
        return { deleteNow: 0, deleteLater: 0, days: null };
      case 'held_audio':
        return { count: 0, impact: { deleteNow: 0, deleteLater: 0, days: 0 } };
      case 'budget_status':
        return budget;
      case 'list_providers':
        return providers;
      case 'task_models':
        return [];
      case 'model_status':
        return modelStatus;
      case 'startup_notices':
        return [];
      case 'input_devices':
        return { devices: [{ id: 'mic-1', name: 'Built-in Microphone' }], defaultId: 'mic-1' };
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

function renderSettings() {
  const user = userEvent.setup();
  render(
    <I18nProvider locale="en">
      <SettingsScreen />
    </I18nProvider>,
  );
  return user;
}

/** The section list, once its tabs are there: React Aria fills it in a second render. */
async function sections() {
  const list = await screen.findByRole('tablist', { name: 'Settings sections' });
  await within(list).findAllByRole('tab');
  return list;
}

afterEach(() => {
  clearMocks();
});

describe('SettingsScreen', () => {
  it('shows the phase 1 sections and hides Claude (MCP)', async () => {
    mockShell();
    const user = renderSettings();
    const tabs = within(await sections()).getAllByRole('tab');
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      'Providers and models',
      'Transcription',
      'Recording',
      'Data and privacy',
      'Appearance',
      'General',
    ]);
    expect(screen.queryByRole('tab', { name: 'Claude (MCP)' })).not.toBeInTheDocument();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    expect(await screen.findByRole('radiogroup', { name: 'Keep meeting audio' })).toBeVisible();
    expect(screen.getByText(appSettings.library.current)).toBeInTheDocument();

    await user.click(screen.getByRole('tab', { name: 'Transcription' }));
    expect(await screen.findByText('Not downloaded')).toBeInTheDocument();

    await user.click(screen.getByRole('tab', { name: 'Recording' }));
    expect(await screen.findByText('⌥⌘R')).toBeInTheDocument();
  });

  it('saves the theme and the audio retention', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Appearance' }));
    await user.click(await screen.findByRole('radio', { name: 'Dark' }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        cmd: 'set_appearance',
        args: { theme: 'dark', accent: 'lime' },
      }),
    );
    expect(screen.getByRole('radio', { name: 'Dark' })).toBeChecked();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    await user.click(await screen.findByRole('radio', { name: /^30 days/ }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        cmd: 'set_library_settings',
        args: { settings: { ...librarySettings, audioRetention: '30_days' } },
      }),
    );
  });

  it('asks before a retention change deletes existing audio, and Cancel keeps it', async () => {
    const calls = mockShell({
      audio_retention_impact: () => ({ deleteNow: 3, deleteLater: 1, days: 7 }),
    });
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    await user.click(await screen.findByRole('radio', { name: /^7 days/ }));
    const dialog = await screen.findByRole('alertdialog', {
      name: 'Delete audio you already have?',
    });
    expect(calls).toContainEqual({ cmd: 'audio_retention_impact', args: { retention: '7_days' } });
    expect(
      within(dialog).getByText('The audio of 3 recordings will be deleted now.'),
    ).toBeVisible();
    expect(
      within(dialog).getByText(
        'The audio of 1 recording will be deleted 7 days after its transcription.',
      ),
    ).toBeVisible();
    expect(within(dialog).getByText(/this can't be undone/)).toBeVisible();

    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
    expect(calls.some((call) => call.cmd === 'set_library_settings')).toBe(false);
    expect(screen.getByRole('radio', { name: /^Never/ })).toBeChecked();
  });

  it('saves a retention change that deletes audio once confirmed', async () => {
    const calls = mockShell({
      library_settings: () => ({ ...librarySettings, audioRetention: 'always' }),
      audio_retention_impact: () => ({ deleteNow: 2, deleteLater: 0, days: 0 }),
    });
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    await user.click(await screen.findByRole('radio', { name: /^Never/ }));
    const dialog = await screen.findByRole('alertdialog');
    expect(
      within(dialog).getByText('The audio of 2 recordings will be deleted now.'),
    ).toBeVisible();
    await user.click(within(dialog).getByRole('button', { name: 'Delete audio' }));

    await waitFor(() =>
      expect(calls).toContainEqual({
        cmd: 'set_library_settings',
        args: {
          settings: { ...librarySettings, audioRetention: 'never' },
          confirmAudioDeletion: true,
        },
      }),
    );
    expect(screen.getByRole('radio', { name: /^Never/ })).toBeChecked();
  });

  it('saves a retention change that deletes nothing without asking', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    await user.click(await screen.findByRole('radio', { name: /^Always/ }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        cmd: 'set_library_settings',
        args: { settings: { ...librarySettings, audioRetention: 'always' } },
      }),
    );
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  });

  it('keeps audio from before the update until the user applies the setting to it', async () => {
    let held = 4;
    const calls = mockShell({
      held_audio: () => ({
        count: held,
        impact: { deleteNow: held, deleteLater: 0, days: 0 },
      }),
      apply_retention_to_held_audio: () => {
        held = 0;
        return { count: 0, impact: { deleteNow: 0, deleteLater: 0, days: 0 } };
      },
    });
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    expect(
      await screen.findByText(/^4 recordings from before this update still have their audio/),
    ).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Apply the setting to them' }));
    const dialog = await screen.findByRole('alertdialog');
    expect(
      within(dialog).getByText('The audio of 4 recordings will be deleted now.'),
    ).toBeVisible();

    // Cancel: nothing is applied.
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
    expect(calls.some((call) => call.cmd === 'apply_retention_to_held_audio')).toBe(false);

    await user.click(screen.getByRole('button', { name: 'Apply the setting to them' }));
    await user.click(
      within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Delete audio' }),
    );
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: 'apply_retention_to_held_audio', args: {} }),
    );
    await waitFor(() =>
      expect(screen.queryByText(/recordings from before this update/)).not.toBeInTheDocument(),
    );
  });

  it('says nothing about audio from before the update that the setting keeps', async () => {
    mockShell({
      held_audio: () => ({ count: 2, impact: { deleteNow: 0, deleteLater: 0, days: null } }),
    });
    const user = renderSettings();
    await sections();
    await user.click(screen.getByRole('tab', { name: 'Data and privacy' }));
    await screen.findByRole('radiogroup', { name: 'Keep meeting audio' });
    expect(screen.queryByText(/from before this update/)).not.toBeInTheDocument();
  });

  it('saves Reduce transparency and makes the window opaque', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Appearance' }));
    const toggle = await screen.findByRole('switch', { name: /Reduce transparency/ });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: 'set_reduce_transparency', args: { enabled: true } }),
    );
    expect(toggle).toBeChecked();
  });

  it('saves Reduce motion and calms the interface', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Appearance' }));
    const toggle = await screen.findByRole('switch', { name: /Reduce motion/ });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: 'set_reduce_motion', args: { enabled: true } }),
    );
    expect(toggle).toBeChecked();
  });

  it('translates an error code and undoes the change', async () => {
    mockShell({
      set_appearance: () => {
        throw 'unknown_appearance';
      },
    });
    const user = renderSettings();
    await sections();

    await user.click(screen.getByRole('tab', { name: 'Appearance' }));
    await user.click(await screen.findByRole('radio', { name: 'Light' }));
    expect(await screen.findByRole('alert')).toHaveTextContent("This appearance isn't available.");
    expect(screen.getByRole('radio', { name: 'System' })).toBeChecked();
  });

  it('adds a provider and never shows its key again', async () => {
    const calls = mockShell({
      add_provider: (args) => {
        const input = args.input as { name: string; baseUrl: string; needsKey: boolean };
        return {
          id: '0190b1c2-0000-7000-8000-000000000001',
          name: input.name,
          baseUrl: input.baseUrl,
          needsKey: input.needsKey,
          hasKey: true,
          catalogFetchedAt: null,
          models: [],
        } satisfies Provider;
      },
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Providers and models' }));
    await user.click(await screen.findByRole('button', { name: 'Add provider' }));

    const dialog = await screen.findByRole('dialog', { name: 'Add a provider' });
    await user.type(within(dialog).getByLabelText('Name'), 'Local server');
    await user.type(within(dialog).getByLabelText('Address'), 'http://localhost:11434/v1');
    const key = within(dialog).getByLabelText('API key');
    expect(key).toHaveAttribute('type', 'password');
    await user.type(key, FAKE_KEY);
    await user.click(within(dialog).getByRole('button', { name: 'Add' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(calls).toContainEqual({
      cmd: 'add_provider',
      args: {
        input: {
          name: 'Local server',
          baseUrl: 'http://localhost:11434/v1',
          needsKey: true,
          apiKey: FAKE_KEY,
        },
      },
    });
    expect(await screen.findByText('Local server')).toBeInTheDocument();
    expect(screen.getByText('Key in the Keychain')).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain(FAKE_KEY);

    // Editing shows an empty key field: the saved key is never read back.
    await user.click(screen.getByRole('button', { name: 'Edit Local server' }));
    const edit = await screen.findByRole('dialog', { name: 'Edit Local server' });
    expect(within(edit).getByLabelText('API key')).toHaveValue('');
    expect(document.body.innerHTML).not.toContain(FAKE_KEY);
  });

  it('shows the error state when the settings cannot be read', async () => {
    mockShell({
      app_settings: () => {
        throw 'storage';
      },
    });
    renderSettings();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Trama couldn't read or save the settings in the library.",
    );
  });
});

describe('SettingsScreen safeguards', () => {
  it('shows what went wrong at start, even without a library', async () => {
    mockShell({
      startup_notices: () => [
        { code: 'library_failed', path: '/Volumes/Gone/Trama' },
        { code: 'settings_reset', path: '/Users/test/settings.json' },
      ],
      app_settings: () => {
        throw 'storage';
      },
    });
    renderSettings();
    expect(
      await screen.findByText(/No library could be opened in .*\/Volumes\/Gone\/Trama/),
    ).toBeInTheDocument();
    expect(screen.getByText(/settings\.json\.corrupt/)).toBeInTheDocument();
  });

  it('warns that an enforced budget refuses models without a price', async () => {
    const provider: Provider = {
      id: '0190b1c2-0000-7000-8000-000000000002',
      name: 'Gateway',
      baseUrl: 'https://gateway.example.com/v1',
      needsKey: true,
      hasKey: true,
      catalogFetchedAt: null,
      models: [
        {
          modelId: 'small-model',
          listed: true,
          catalogContextTokens: 128_000,
          catalogMaxOutputTokens: 4096,
          contextTokensOverride: null,
          maxOutputTokensOverride: null,
          inputPriceMicros: 150_000,
          outputPriceMicros: null,
        },
      ],
    };
    mockShell({
      library_settings: () => ({
        ...librarySettings,
        monthlyBudgetMicros: 5_000_000,
        enforceBudget: true,
      }),
      list_providers: () => [provider],
      task_models: () => [{ task: 'summary', providerId: provider.id, modelId: 'small-model' }],
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Providers and models' }));
    expect(
      await screen.findByText(/small-model has no price, so with the limit on/),
    ).toBeInTheDocument();
  });

  it('ignores an older answer that arrives after a newer change', async () => {
    const answers: ((value: unknown) => void)[] = [];
    mockShell({
      set_library_settings: () =>
        new Promise((resolve) => {
          answers.push(resolve);
        }),
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Data and privacy' }));
    await user.click(await screen.findByRole('radio', { name: /^7 days/ }));
    await user.click(screen.getByRole('radio', { name: /^Always/ }));
    await waitFor(() => expect(answers).toHaveLength(2));
    answers[1]?.({ ...librarySettings, audioRetention: 'always' });
    answers[0]?.({ ...librarySettings, audioRetention: '7_days' });
    await waitFor(() => expect(screen.getByRole('radio', { name: /^Always/ })).toBeChecked());
  });

  it('lets Tab leave the shortcut recorder', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Recording' }));
    await user.click(await screen.findByRole('button', { name: /^Change shortcut/ }));
    expect(screen.getByText(/Press the new shortcut/)).toBeInTheDocument();
    await user.tab();
    expect(screen.queryByText(/Press the new shortcut/)).not.toBeInTheDocument();
    expect(calls.some((call) => call.cmd === 'set_global_shortcut')).toBe(false);
  });
});

describe('global shortcut and startup', () => {
  it('says when the stored shortcut is not listening, until another is saved', async () => {
    const calls = mockShell({
      app_settings: () => ({ ...appSettings, globalShortcutError: 'shortcut_taken' }),
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Recording' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/another app may be using it/);
    await user.click(screen.getByRole('button', { name: /^Change shortcut/ }));
    await user.keyboard('{Control>}{Alt>}m{/Alt}{/Control}');
    await waitFor(() =>
      expect(calls.find((call) => call.cmd === 'set_global_shortcut')?.args).toEqual({
        shortcut: 'Control+Alt+KeyM',
      }),
    );
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: /^Change shortcut: ⌃⌥M/ })).toBeInTheDocument();
  });

  it('keeps the shortcut when macOS or Trama already uses the new one', async () => {
    mockShell({
      // Commands reject with a code.
      set_global_shortcut: () => {
        throw 'shortcut_reserved';
      },
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'Recording' }));
    await user.click(await screen.findByRole('button', { name: /^Change shortcut/ }));
    await user.keyboard('{Meta>}n{/Meta}');
    expect(await screen.findByRole('alert')).toHaveTextContent(/already uses this shortcut/);
    expect(screen.getByRole('button', { name: /^Change shortcut: ⌥⌘R/ })).toBeInTheDocument();
  });

  it('turns opening at login on and off', async () => {
    const calls = mockShell();
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'General' }));
    const toggle = await screen.findByRole('switch', { name: /Open at login/ });
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(calls.find((call) => call.cmd === 'set_open_at_login')?.args).toEqual({
      enabled: true,
    });
  });

  it('says when opening at login cannot change', async () => {
    mockShell({
      set_open_at_login: () => {
        throw 'login_item_failed';
      },
    });
    const user = renderSettings();
    await user.click(await screen.findByRole('tab', { name: 'General' }));
    const toggle = await screen.findByRole('switch', { name: /Open at login/ });
    await waitFor(() => expect(toggle).toBeEnabled());
    await user.click(toggle);
    expect(await screen.findByRole('alert')).toHaveTextContent(/Login Items/);
    expect(toggle).not.toBeChecked();
  });
});

describe('global shortcut', () => {
  const key = (code: string, mods: Partial<KeyboardEventInit> = {}) =>
    new KeyboardEvent('keydown', { code, ...mods });
  const t = createTranslate(createBundle('en'));

  it('reads a key event as the core stores it', () => {
    expect(shortcutFromEvent(key('KeyR', { altKey: true, metaKey: true }))).toBe('Alt+Super+KeyR');
    expect(shortcutFromEvent(key('Digit1', { ctrlKey: true, shiftKey: true }))).toBe(
      'Control+Shift+Digit1',
    );
    expect(shortcutFromEvent(key('Space', { metaKey: true }))).toBe('Super+Space');
    // A lone modifier is not a shortcut yet.
    expect(shortcutFromEvent(key('MetaLeft', { metaKey: true }))).toBeNull();
  });

  it('writes a shortcut with the macOS symbols', () => {
    expect(formatShortcut('Alt+Super+KeyR', t)).toBe('⌥⌘R');
    expect(formatShortcut('Control+Shift+Digit1', t)).toBe('⌃⇧1');
    expect(formatShortcut('Super+Slash', t)).toBe('⌘/');
    expect(formatShortcut('Super+Space', t)).toBe('⌘Space');
  });
});

/**
 * Codes the shell's settings commands return (see `trama_core::config::config_error`,
 * `storage::provider_input_error`, `models::ModelError::code`, `capture::error_code` and
 * `src-tauri/src/settings.rs`). Each needs a message in every language.
 */
const SETTINGS_CODES = [
  'internal',
  'storage',
  'invalid_setting',
  'config_write',
  'unsupported_language',
  'unknown_appearance',
  'invalid_shortcut',
  'shortcut_reserved',
  'shortcut_taken',
  'login_item_failed',
  'library_not_absolute',
  'library_not_a_folder',
  'library_folder_not_empty',
  'library_synced',
  'name_required',
  'url_scheme',
  'url_invalid',
  'url_insecure',
  'model_not_enough_space',
  'model_network',
  'model_http_status',
  'model_size_mismatch',
  'model_checksum_mismatch',
  'model_cancelled',
  'model_io',
  'model_panicked',
  'capture_no_input_device',
  'capture_device_open_failed',
  'library_not_writable',
  'audio_deletion_unconfirmed',
];
const WARNINGS = ['library_maybe_synced', 'library_exists'];
const NOTICES = ['settings_reset', 'settings_unreadable', 'library_unavailable', 'library_failed'];

describe('error messages', () => {
  it.each(SUPPORTED_LOCALES)('has a message for every code in %s', (locale) => {
    const bundle = createBundle(locale);
    for (const code of SETTINGS_CODES) {
      expect(bundle.hasMessage(`settings-error-${code.replace(/_/g, '-')}`), code).toBe(true);
    }
    for (const code of NOTICES) {
      expect(bundle.hasMessage(`settings-notice-${code.replace(/_/g, '-')}`), code).toBe(true);
    }
    expect(bundle.hasMessage('provider-error-price-missing')).toBe(true);
    for (const code of WARNINGS) {
      expect(bundle.hasMessage(`settings-warning-${code.replace(/_/g, '-')}`), code).toBe(true);
    }
  });
});
