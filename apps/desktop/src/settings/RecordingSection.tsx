import { useCallback, useEffect, useState } from 'react';
import { Button, Radio, RadioGroup } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import type { Translate } from '../i18n/catalogs';
import { ArrowsClockwiseIcon, MicrophoneIcon } from '../ui/icons';
import { api, errorCode, type AppSettings, type InputDevices } from './api';
import { ErrorText, Group, Row } from './ui';

const SYSTEM_DEFAULT = '';

/** Modifiers of a stored shortcut, in order, with their macOS symbols. */
const MODIFIERS = [
  { name: 'Control', symbol: '⌃', pressed: (e: KeyboardEvent) => e.ctrlKey },
  { name: 'Alt', symbol: '⌥', pressed: (e: KeyboardEvent) => e.altKey },
  { name: 'Shift', symbol: '⇧', pressed: (e: KeyboardEvent) => e.shiftKey },
  { name: 'Super', symbol: '⌘', pressed: (e: KeyboardEvent) => e.metaKey },
] as const;

const PUNCTUATION: Record<string, string> = {
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backquote: '`',
};

/** The shortcut of a key event as the core stores it (`Alt+Super+KeyR`), or `null` for a lone modifier. */
export function shortcutFromEvent(event: KeyboardEvent): string | null {
  const code = event.code;
  const isKey =
    /^Key[A-Z]$/.test(code) ||
    /^Digit[0-9]$/.test(code) ||
    /^F([1-9]|1[0-9]|20)$/.test(code) ||
    code === 'Space' ||
    code in PUNCTUATION;
  if (!isKey) return null;
  const modifiers = MODIFIERS.filter((m) => m.pressed(event)).map((m) => m.name);
  return [...modifiers, code].join('+');
}

/** A stored shortcut written with the macOS symbols, for example ⌥⌘R. */
export function formatShortcut(shortcut: string, t: Translate): string {
  const parts = shortcut.split('+');
  const key = parts.pop() ?? '';
  const symbols = MODIFIERS.filter((m) => parts.includes(m.name)).map((m) => m.symbol);
  const label =
    key.match(/^Key([A-Z])$/)?.[1] ??
    key.match(/^Digit([0-9])$/)?.[1] ??
    PUNCTUATION[key] ??
    (key === 'Space' ? t('settings-key-space') : key);
  return [...symbols, label].join('');
}

interface Props {
  app: AppSettings;
  onApp: (app: AppSettings) => void;
}

export function RecordingSection({ app, onApp }: Props) {
  const { t } = useI18n();
  const [devices, setDevices] = useState<InputDevices | null>(null);
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const [micError, setMicError] = useState<string | null>(null);
  const [recording, setRecording] = useState(false);
  const [shortcutError, setShortcutError] = useState<string | null>(null);

  const loadDevices = useCallback(async () => {
    setDevicesError(null);
    try {
      setDevices(await api.inputDevices());
    } catch (err) {
      setDevicesError(`capture_${errorCode(err)}`);
    }
  }, []);

  useEffect(() => {
    // Loading from the core, which answers asynchronously.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadDevices();
  }, [loadDevices]);

  const saveShortcut = useCallback(
    async (shortcut: string) => {
      setShortcutError(null);
      try {
        await api.setGlobalShortcut(shortcut);
        onApp({ ...app, globalShortcut: shortcut });
      } catch (err) {
        setShortcutError(errorCode(err));
      }
    },
    [app, onApp],
  );

  // While recording, the next key press with a modifier becomes the shortcut; Escape cancels.
  // Tab still moves the focus, so the keyboard is never trapped.
  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Tab') {
        setRecording(false);
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      if (event.key === 'Escape') {
        setRecording(false);
        return;
      }
      const shortcut = shortcutFromEvent(event);
      if (shortcut === null) return;
      setRecording(false);
      void saveShortcut(shortcut);
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [recording, saveShortcut]);

  const defaultName = devices?.devices.find((d) => d.id === devices.defaultId)?.name;
  const options = [
    {
      id: SYSTEM_DEFAULT,
      label: t('settings-microphone-default'),
      detail: defaultName,
    },
    ...(devices?.devices ?? []).map((device) => ({
      id: device.id,
      label: device.name,
      detail: undefined as string | undefined,
    })),
  ];
  // A chosen microphone that is not connected now is still shown, so the choice isn't lost.
  if (app.microphone && !options.some((option) => option.id === app.microphone)) {
    options.push({
      id: app.microphone,
      label: t('settings-microphone-missing'),
      detail: undefined,
    });
  }

  const changeMicrophone = async (value: string) => {
    const device = value === SYSTEM_DEFAULT ? null : value;
    setMicError(null);
    try {
      await api.setMicrophone(device);
      onApp({ ...app, microphone: device });
    } catch (err) {
      setMicError(errorCode(err));
    }
  };

  return (
    <>
      <Group
        title={t('settings-microphone')}
        description={t('settings-microphone-desc')}
        action={
          <Button className="set-btn" data-variant="quiet" onPress={() => void loadDevices()}>
            <ArrowsClockwiseIcon aria-hidden />
            {t('settings-microphone-refresh')}
          </Button>
        }
      >
        <RadioGroup
          className="set-devices"
          aria-label={t('settings-microphone')}
          value={app.microphone ?? SYSTEM_DEFAULT}
          onChange={(value) => void changeMicrophone(value)}
        >
          {options.map((option) => (
            <Radio key={option.id} value={option.id} className="set-device">
              <span className="set-radio" aria-hidden />
              <MicrophoneIcon className="set-device-ic" aria-hidden />
              <span className="set-device-name">{option.label}</span>
              {option.detail && <span className="set-mono set-device-detail">{option.detail}</span>}
            </Radio>
          ))}
        </RadioGroup>
        <ErrorText code={micError} />
        <ErrorText code={devicesError} />
      </Group>

      <Group title={t('settings-group-shortcut')}>
        <Row label={t('settings-shortcut')} description={t('settings-shortcut-desc')}>
          <div className="set-shortcut">
            <Button
              className="set-keycap"
              data-recording={recording || undefined}
              aria-label={
                recording
                  ? t('settings-shortcut-stop')
                  : `${t('settings-shortcut-change')}: ${formatShortcut(app.globalShortcut, t)}`
              }
              onPress={() => setRecording((on) => !on)}
            >
              {recording ? t('settings-shortcut-press') : formatShortcut(app.globalShortcut, t)}
            </Button>
            {app.globalShortcut !== app.defaultGlobalShortcut && (
              <Button
                className="set-link"
                onPress={() => void saveShortcut(app.defaultGlobalShortcut)}
              >
                {t('settings-shortcut-reset')}
              </Button>
            )}
          </div>
        </Row>
        <div role="status" className="set-recording-hint">
          {recording && <p className="set-row-desc">{t('settings-shortcut-recording')}</p>}
        </div>
        <ErrorText code={shortcutError} />
        <p className="set-row-desc set-note">{t('settings-shortcut-note')}</p>
      </Group>
    </>
  );
}
