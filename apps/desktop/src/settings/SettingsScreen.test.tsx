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
  microphone: null,
  globalShortcut: 'Alt+Super+KeyR',
  defaultGlobalShortcut: 'Alt+Super+KeyR',
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
      case 'budget_status':
        return budget;
      case 'list_providers':
        return providers;
      case 'task_models':
        return [];
      case 'model_status':
        return modelStatus;
      case 'input_devices':
        return { devices: [{ id: 'mic-1', name: 'Built-in Microphone' }], defaultId: 'mic-1' };
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

const sections = () => screen.findByRole('tablist', { name: 'Settings sections' });

afterEach(() => {
  clearMocks();
});

describe('SettingsScreen', () => {
  it('shows the phase 1 sections and hides Claude (MCP)', async () => {
    mockShell();
    const user = renderSettings();
    const tabs = within(await sections()).getAllByRole('tab');
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      'General',
      'Providers and models',
      'Transcription',
      'Recording',
      'Data and privacy',
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

  it('translates an error code and undoes the change', async () => {
    mockShell({
      set_appearance: () => {
        throw 'unknown_appearance';
      },
    });
    const user = renderSettings();
    await sections();

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
];
const WARNINGS = ['library_maybe_synced', 'library_exists'];

describe('error messages', () => {
  it.each(SUPPORTED_LOCALES)('has a message for every code in %s', (locale) => {
    const bundle = createBundle(locale);
    for (const code of SETTINGS_CODES) {
      expect(bundle.hasMessage(`settings-error-${code.replace(/_/g, '-')}`), code).toBe(true);
    }
    for (const code of WARNINGS) {
      expect(bundle.hasMessage(`settings-warning-${code.replace(/_/g, '-')}`), code).toBe(true);
    }
  });
});
