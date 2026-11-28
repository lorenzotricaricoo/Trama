import { AnimatePresence, MotionConfig, motion, type Variants } from 'motion/react';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Button } from 'react-aria-components';
import { useAppearance } from '../appearance';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { useFlushBeforeQuit } from '../notes/flush';
import { AccentPicker } from '../settings/AppearanceSection';
import { api, errorCode, type AppSettings, type LibrarySettings } from '../settings/api';
import { languageName, OpenAtLoginToggle } from '../settings/GeneralSection';
import { ProvidersGroup, TasksGroup, useProviders } from '../settings/ProvidersSection';
import { formatShortcut, ShortcutField } from '../settings/RecordingSection';
import '../settings/settings.css';
import {
  ModelCard,
  ModelFacts,
  useMeetingLanguageOptions,
  useModelStatus,
} from '../settings/TranscriptionSection';
import { ErrorText, Group, SelectField } from '../settings/ui';
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  CheckIcon,
  DesktopIcon,
  InfoIcon,
  MicrophoneIcon,
  NotePencilIcon,
  RecordIcon,
  SparkleIcon,
  SpeakerHighIcon,
  WarningIcon,
  WaveformIcon,
  type Icon,
} from '../ui/icons';
import { useThemeClasses } from '../useThemeClasses';
import { onboardingApi, type Permission, type PrivacyPane } from './api';
import './onboarding.css';
import { DONE, nextStep, previousStep, STEP_ICONS, STEPS, stepIndex, type Step } from './steps';
import trayIcon from '../../src-tauri/icons/tray/tray.png';

/** The Dock icons of each accent (Ordito), as on macOS while Trama is open. */
const APP_ICONS = import.meta.glob<string>('../../src-tauri/icons/dock/*.png', {
  eager: true,
  query: '?url',
  import: 'default',
});

/** The app icon in the accent chosen now. */
function AppIcon({ className }: { className: string }) {
  const { accent } = useAppearance();
  const src = APP_ICONS[`../../src-tauri/icons/dock/${accent}.png`];
  return src ? <img className={className} src={src} alt="" aria-hidden draggable={false} /> : null;
}

// A step enters from the direction of travel and leaves quickly the other way.
const stage: Variants = {
  enter: (forward: boolean) => ({ opacity: 0, x: forward ? 18 : -18 }),
  center: { opacity: 1, x: 0, transition: { duration: duration.d4, ease: ease.out } },
  exit: (forward: boolean) => ({
    opacity: 0,
    x: forward ? -10 : 10,
    transition: { duration: duration.d1, ease: ease.in },
  }),
};

const list: Variants = { center: { transition: { staggerChildren: 0.05, delayChildren: 0.06 } } };
const rise: Variants = {
  enter: { opacity: 0, y: 10 },
  center: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** Saves a change of the library settings; resolves to an error code, or `null`. */
type UpdateLibrary = (change: Partial<LibrarySettings>) => Promise<string | null>;

/** What the steps share: the permissions found so far, and the settings being changed. */
interface Permissions {
  microphone: Permission | null;
  /** `null` until checked: macOS has no way to read it without the probe. */
  systemAudio: Permission | null;
}

/**
 * First launch (flow F1): the meeting language, the permissions, the provider, the
 * transcription model and the menu bar, one step at a time. Every step can wait: the last
 * one says what is still missing, and Home keeps a reminder until it is set up.
 */
export function Onboarding({
  initialStep,
  onFinish,
}: {
  initialStep: Step;
  /** The onboarding was finished or skipped, and stored as done. */
  onFinish: () => void;
}) {
  const { t } = useI18n();
  useThemeClasses();
  const { reduceMotion } = useAppearance();
  // Nothing to save while onboarding: quitting answers at once.
  useFlushBeforeQuit(() => {});

  const [step, setStep] = useState<Step>(initialStep);
  const [forward, setForward] = useState(true);
  // The furthest step reached: the ones before it count as seen.
  const [furthest, setFurthest] = useState(stepIndex(initialStep));
  const [app, setApp] = useState<AppSettings | null>(null);
  const [library, setLibrary] = useState<LibrarySettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [permissions, setPermissions] = useState<Permissions>({
    microphone: null,
    systemAudio: null,
  });
  const providers = useProviders();
  const model = useModelStatus().status;
  const libraryRequest = useRef(0);

  useEffect(() => {
    let active = true;
    Promise.all([api.appSettings(), api.librarySettings()])
      .then(([appSettings, librarySettings]) => {
        if (!active) return;
        setApp(appSettings);
        setLibrary(librarySettings);
      })
      .catch((error: unknown) => {
        if (active) setLoadError(errorCode(error));
      });
    onboardingApi
      .microphone()
      .then((microphone) => {
        if (active) setPermissions((current) => ({ ...current, microphone }));
      })
      .catch((error: unknown) => console.error('Cannot read the microphone permission', error));
    return () => {
      active = false;
    };
  }, []);

  const updateLibrary: UpdateLibrary = useCallback(
    async (change) => {
      if (!library) return null;
      const request = ++libraryRequest.current;
      const previous = library;
      setLibrary({ ...library, ...change });
      try {
        const saved = await api.setLibrarySettings({ ...library, ...change });
        if (request === libraryRequest.current) setLibrary(saved);
        return null;
      } catch (error) {
        if (request === libraryRequest.current) setLibrary(previous);
        return errorCode(error);
      }
    },
    [library],
  );

  const store = (next: string) => {
    onboardingApi.setStep(next).catch((error: unknown) => {
      console.error('Cannot record the onboarding step', error);
    });
  };

  const go = (next: Step) => {
    if (next === step) return;
    setForward(stepIndex(next) > stepIndex(step));
    setFurthest((reached) => Math.max(reached, stepIndex(next)));
    setStep(next);
    store(next);
  };

  // Stored as done before leaving: an answer that never comes must not trap the user here.
  const finish = () => {
    store(DONE);
    onFinish();
  };

  const summaryChosen = providers.tasks.some((task) => task.task === 'summary');
  const modelBusy = model?.state === 'downloading' || model?.state === 'verifying';
  const complete: Record<Step, boolean> = {
    welcome: furthest > 0,
    language: furthest > stepIndex('language'),
    permissions: permissions.microphone === 'granted' && permissions.systemAudio === 'granted',
    provider: summaryChosen,
    model: model?.state === 'ready',
    everyday: furthest > stepIndex('everyday'),
    ready: false,
  };
  // A step whose work goes on by itself (the download) lets the user go on without "Not now".
  const canContinue =
    complete[step] || (step === 'model' && modelBusy) || step === 'language' || step === 'everyday';

  // What recording a meeting still lacks. System audio is not counted: macOS can't say
  // whether it is allowed until it is checked, and a fresh start doesn't remember the check.
  const leftForLater =
    permissions.microphone !== 'granted' || !summaryChosen || model?.state !== 'ready';
  const index = stepIndex(step);
  const total = STEPS.length;

  let content: ReactNode;
  if (loadError !== null) {
    content = (
      <div className="onb-load">
        <ErrorText code={loadError} />
      </div>
    );
  } else if (app === null || library === null) {
    content = (
      <p className="onb-load" role="status">
        {t('settings-loading')}
      </p>
    );
  } else {
    switch (step) {
      case 'welcome':
        content = <WelcomeStep app={app} onApp={setApp} />;
        break;
      case 'language':
        content = <LanguageStep library={library} onLibrary={updateLibrary} />;
        break;
      case 'permissions':
        content = <PermissionsStep permissions={permissions} onChange={setPermissions} />;
        break;
      case 'provider':
        content = <ProviderStep state={providers} currency={library.currency} />;
        break;
      case 'model':
        content = <ModelStep />;
        break;
      case 'everyday':
        content = <EverydayStep app={app} onApp={setApp} />;
        break;
      case 'ready':
        content = (
          <ReadyStep
            app={app}
            library={library}
            permissions={permissions}
            summaryChosen={summaryChosen}
            model={model}
            onGo={go}
          />
        );
        break;
    }
  }

  return (
    <MotionConfig reducedMotion={reduceMotion ? 'always' : 'user'}>
      <div className="app onb">
        <div className="titlebar" data-tauri-drag-region />
        <nav className="side onb-rail" aria-label={t('onboarding-steps')}>
          <div className="side-head" />
          <div className="onb-brand">
            <AppIcon className="onb-brand-mark" />
            <span className="onb-brand-name">{t('onboarding-brand')}</span>
          </div>
          <ol className="onb-steps">
            {STEPS.map((id, position) => {
              const StepIcon = STEP_ICONS[id];
              const current = id === step;
              const done = complete[id];
              const later = !done && !current && position < furthest && id !== 'ready';
              return (
                <li key={id}>
                  <Button
                    className="side-item onb-step"
                    aria-current={current ? 'step' : undefined}
                    data-done={done || undefined}
                    onPress={() => go(id)}
                  >
                    {current && (
                      <motion.span
                        layoutId="onb-ind"
                        className="side-ind"
                        transition={{ type: 'spring', stiffness: 420, damping: 36 }}
                      />
                    )}
                    <span className="onb-step-mark" aria-hidden>
                      {done ? <CheckIcon weight="bold" /> : <StepIcon />}
                    </span>
                    <span className="side-label">{t(`onboarding-step-${id}`)}</span>
                    {id === 'model' && modelBusy && model.progress !== null ? (
                      <span className="onb-step-meta">{Math.round(model.progress * 100)}%</span>
                    ) : (
                      later && <span className="onb-step-meta">{t('onboarding-later-mark')}</span>
                    )}
                    {done && <span className="visually-hidden">{t('onboarding-step-done')}</span>}
                  </Button>
                </li>
              );
            })}
          </ol>
          <div className="side-grow" />
          {step !== 'ready' && (
            <div className="onb-skip">
              <Button className="onb-skip-btn" onPress={finish}>
                {t('onboarding-skip')}
              </Button>
              <p className="onb-skip-hint">{t('onboarding-skip-hint')}</p>
            </div>
          )}
        </nav>
        <main className="sheet onb-sheet">
          <div className="stage onb-stage">
            <AnimatePresence mode="wait" initial={false} custom={forward}>
              <motion.div
                key={step}
                className="onb-page"
                custom={forward}
                variants={stage}
                initial="enter"
                animate="center"
                exit="exit"
              >
                <motion.div className="onb-content" variants={list}>
                  <motion.header className="onb-head" variants={rise}>
                    {step === 'welcome' && <AppIcon className="onb-hero" />}
                    <div className="kicker onb-kicker">
                      {t('onboarding-step-count', { current: index + 1, total })}
                    </div>
                    <h1 className="screen-title">
                      {t(`onboarding-${step}-title`, titleArgs(step, leftForLater))}
                    </h1>
                    <p className="onb-desc">{stepDescription(step, t, app)}</p>
                  </motion.header>
                  <motion.div className="onb-body" variants={rise}>
                    {content}
                  </motion.div>
                </motion.div>
              </motion.div>
            </AnimatePresence>
          </div>
          <footer className="onb-foot">
            {index > 0 ? (
              <Button
                className="set-btn"
                data-variant="quiet"
                onPress={() => go(previousStep(step))}
              >
                <ArrowLeftIcon aria-hidden />
                {t('onboarding-back')}
              </Button>
            ) : (
              <span />
            )}
            <div className="onb-foot-end">
              {step === 'ready' ? (
                <Button className="set-btn onb-next" data-variant="primary" onPress={finish}>
                  {t('onboarding-finish')}
                  <ArrowRightIcon aria-hidden />
                </Button>
              ) : step === 'welcome' ? (
                <Button
                  className="set-btn onb-next"
                  data-variant="primary"
                  onPress={() => go(nextStep(step))}
                >
                  {t('onboarding-start')}
                  <ArrowRightIcon aria-hidden />
                </Button>
              ) : canContinue ? (
                <Button
                  className="set-btn onb-next"
                  data-variant="primary"
                  onPress={() => go(nextStep(step))}
                >
                  {t('onboarding-continue')}
                  <ArrowRightIcon aria-hidden />
                </Button>
              ) : (
                <Button className="set-btn onb-next" onPress={() => go(nextStep(step))}>
                  {t('onboarding-later')}
                  <ArrowRightIcon aria-hidden />
                </Button>
              )}
            </div>
          </footer>
        </main>
      </div>
    </MotionConfig>
  );
}

/** The last step says whether something was left for later. */
function titleArgs(step: Step, missing: boolean) {
  return step === 'ready' ? { state: missing ? 'later' : 'all' } : undefined;
}

function stepDescription(
  step: Step,
  t: ReturnType<typeof useI18n>['t'],
  app: AppSettings | null,
): string {
  if (step === 'ready') {
    return t('onboarding-ready-desc', {
      shortcut: app ? formatShortcut(app.globalShortcut, t) : '⌥⌘R',
    });
  }
  return t(`onboarding-${step}-desc`);
}

/** A short note under a step: what happens if it waits. */
function Note({ children, tone }: { children: ReactNode; tone?: 'warn' }) {
  return (
    <p className="onb-note" data-tone={tone}>
      {tone === 'warn' ? <WarningIcon aria-hidden /> : <InfoIcon aria-hidden />}
      <span>{children}</span>
    </p>
  );
}

function WelcomeStep({ app, onApp }: { app: AppSettings; onApp: (app: AppSettings) => void }) {
  const { t } = useI18n();
  const points: { id: string; icon: Icon }[] = [
    { id: 'record', icon: RecordIcon },
    { id: 'local', icon: DesktopIcon },
    { id: 'summary', icon: SparkleIcon },
  ];
  return (
    <>
      <ul className="onb-points">
        {points.map(({ id, icon: PointIcon }) => (
          <li key={id} className="onb-point">
            <span className="onb-point-ic" aria-hidden>
              <PointIcon />
            </span>
            <span className="onb-point-text">
              <b>{t(`onboarding-welcome-${id}`)}</b>
              <span>{t(`onboarding-welcome-${id}-desc`)}</span>
            </span>
          </li>
        ))}
      </ul>
      <Group
        title={t('onboarding-welcome-accent')}
        description={t('onboarding-welcome-accent-desc')}
      >
        <AccentPicker app={app} onApp={onApp} preview={false} />
      </Group>
    </>
  );
}

function LanguageStep({
  library,
  onLibrary,
}: {
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}) {
  const { t, locale } = useI18n();
  const options = useMeetingLanguageOptions();
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <Group title={t('onboarding-language-label')}>
        <SelectField
          label={t('onboarding-language-label')}
          labelHidden
          options={options}
          value={library.meetingLanguage}
          onChange={(meetingLanguage) => {
            setError(null);
            void onLibrary({ meetingLanguage }).then(setError);
          }}
        />
        <ErrorText code={error} />
      </Group>
      <Note>
        {t('onboarding-language-note', {
          // Inside a sentence: as Intl writes it ("italiano", "English").
          language:
            new Intl.DisplayNames([locale], { type: 'language' }).of(library.meetingLanguage) ??
            library.meetingLanguage,
        })}
      </Note>
    </>
  );
}

function PermissionsStep({
  permissions,
  onChange,
}: {
  permissions: Permissions;
  onChange: (update: (current: Permissions) => Permissions) => void;
}) {
  const { t } = useI18n();
  const [asking, setAsking] = useState<'microphone' | 'systemAudio' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const ask = async (which: 'microphone' | 'systemAudio', call: () => Promise<Permission>) => {
    setAsking(which);
    setError(null);
    try {
      const state = await call();
      onChange((current) => ({ ...current, [which]: state }));
    } catch (err) {
      setError(errorCode(err));
    } finally {
      setAsking(null);
    }
  };

  const openSettings = (pane: PrivacyPane) => {
    onboardingApi.openPrivacySettings(pane).catch((err: unknown) => setError(errorCode(err)));
  };

  const mic = permissions.microphone;
  const system = permissions.systemAudio;

  return (
    <>
      <div className="onb-perms">
        <PermissionCard
          icon={MicrophoneIcon}
          title={t('onboarding-mic-title')}
          description={t('onboarding-mic-desc')}
          state={mic}
          busy={asking === 'microphone'}
          busyLabel={t('onboarding-perm-asking')}
          allowLabel={t('onboarding-allow-mic')}
          problem={
            mic === 'denied'
              ? t('onboarding-mic-denied')
              : mic === 'restricted'
                ? t('onboarding-mic-restricted')
                : null
          }
          // Asking again after a refusal shows nothing: macOS asks only once.
          onAllow={() => void ask('microphone', onboardingApi.requestMicrophone)}
          onCheck={() => void ask('microphone', onboardingApi.microphone)}
          onOpenSettings={mic === 'restricted' ? undefined : () => openSettings('microphone')}
        />
        <PermissionCard
          icon={SpeakerHighIcon}
          title={t('onboarding-system-title')}
          description={t('onboarding-system-desc')}
          state={system}
          busy={asking === 'systemAudio'}
          busyLabel={t('onboarding-perm-checking')}
          allowLabel={t('onboarding-allow-system')}
          problem={
            system !== null && system !== 'granted' ? t('onboarding-system-not-heard') : null
          }
          onAllow={() => void ask('systemAudio', onboardingApi.checkSystemAudio)}
          onCheck={() => void ask('systemAudio', onboardingApi.checkSystemAudio)}
          onOpenSettings={() => openSettings('system_audio')}
          footnote={t('onboarding-system-probe-note')}
        />
      </div>
      <ErrorText code={error} />
      <Note>{t('onboarding-permissions-later')}</Note>
    </>
  );
}

function PermissionCard({
  icon: CardIcon,
  title,
  description,
  state,
  busy,
  busyLabel,
  allowLabel,
  problem,
  onAllow,
  onCheck,
  onOpenSettings,
  footnote,
}: {
  icon: Icon;
  title: string;
  description: string;
  state: Permission | null;
  busy: boolean;
  busyLabel: string;
  allowLabel: string;
  /** Why the permission is missing after asking; `null` while it was never asked. */
  problem: string | null;
  onAllow: () => void;
  onCheck: () => void;
  onOpenSettings?: () => void;
  footnote?: string;
}) {
  const { t } = useI18n();
  const granted = state === 'granted';
  return (
    <article className="set-card onb-perm" aria-label={title} data-granted={granted || undefined}>
      <div className="onb-perm-head">
        <span className="set-engine-ic" aria-hidden>
          <CardIcon />
        </span>
        <div className="onb-perm-text">
          <h3 className="onb-perm-title">{title}</h3>
          <p className="onb-perm-desc">{description}</p>
        </div>
        <div className="onb-perm-action" role="status">
          {granted ? (
            <span className="set-status" data-tone="ok">
              <span className="set-dot" aria-hidden />
              {t('onboarding-perm-granted')}
            </span>
          ) : problem === null ? (
            <Button className="set-btn" data-variant="primary" onPress={onAllow} isPending={busy}>
              {busy ? busyLabel : allowLabel}
            </Button>
          ) : (
            busy && <span className="set-row-desc">{busyLabel}</span>
          )}
        </div>
      </div>
      {!granted && problem !== null && (
        <div className="onb-perm-problem">
          <p className="set-warn">
            <WarningIcon aria-hidden />
            {problem}
          </p>
          <div className="onb-perm-buttons">
            {onOpenSettings && (
              <Button className="set-btn" onPress={onOpenSettings}>
                {t('onboarding-open-settings')}
              </Button>
            )}
            <Button className="set-btn" data-variant="quiet" onPress={onCheck} isDisabled={busy}>
              {t('onboarding-check-again')}
            </Button>
          </div>
        </div>
      )}
      {footnote && !granted && <p className="onb-perm-foot">{footnote}</p>}
    </article>
  );
}

function ProviderStep({
  state,
  currency,
}: {
  state: ReturnType<typeof useProviders>;
  currency: string;
}) {
  const { t } = useI18n();
  return (
    <>
      <ProvidersGroup
        state={state}
        currency={currency}
        description={t('onboarding-provider-group-desc')}
      />
      {(state.providers?.length ?? 0) > 0 && <TasksGroup state={state} />}
      <Note>{t('onboarding-provider-later')}</Note>
    </>
  );
}

function ModelStep() {
  const { t } = useI18n();
  return (
    <>
      <ModelCard />
      <ModelFacts />
      <Note>{t('onboarding-model-later')}</Note>
    </>
  );
}

function EverydayStep({ app, onApp }: { app: AppSettings; onApp: (app: AppSettings) => void }) {
  const { t } = useI18n();
  const items: { id: string; icon: Icon; shortcut?: string }[] = [
    { id: 'record', icon: RecordIcon, shortcut: formatShortcut(app.globalShortcut, t) },
    { id: 'note', icon: NotePencilIcon },
    { id: 'latest', icon: WaveformIcon },
  ];
  return (
    <>
      <div className="onb-menubar" aria-hidden>
        <div className="onb-menubar-strip">
          <span className="onb-menubar-tray" style={{ maskImage: `url(${trayIcon})` }} />
          <span className="onb-menubar-clock">9:41</span>
        </div>
        <ul className="onb-menubar-menu">
          {items.map(({ id, icon: ItemIcon, shortcut }) => (
            <li key={id}>
              <ItemIcon />
              <span>{t(`onboarding-menubar-${id}`)}</span>
              {shortcut && <span className="kbd">{shortcut}</span>}
            </li>
          ))}
        </ul>
      </div>
      <Group title={t('onboarding-shortcut-title')}>
        <ShortcutField app={app} onApp={onApp} />
      </Group>
      <Group title={t('onboarding-login-title')}>
        <OpenAtLoginToggle />
      </Group>
    </>
  );
}

function ReadyStep({
  app,
  library,
  permissions,
  summaryChosen,
  model,
  onGo,
}: {
  app: AppSettings;
  library: LibrarySettings;
  permissions: Permissions;
  summaryChosen: boolean;
  model: ReturnType<typeof useModelStatus>['status'];
  onGo: (step: Step) => void;
}) {
  const { t, locale } = useI18n();
  const modelValue =
    model?.state === 'ready'
      ? { ok: true, text: t('onboarding-check-model-ready') }
      : (model?.state === 'downloading' || model?.state === 'verifying') && model.progress !== null
        ? {
            ok: false,
            busy: true,
            text: t('onboarding-check-model-downloading', {
              percent: new Intl.NumberFormat(locale, { style: 'percent' }).format(model.progress),
            }),
          }
        : { ok: false, text: t('onboarding-check-model-missing') };
  const rows: { id: string; step: Step; ok: boolean; busy?: boolean; text: string }[] = [
    {
      id: 'language',
      step: 'language',
      ok: true,
      text: languageName(library.meetingLanguage, locale),
    },
    {
      id: 'microphone',
      step: 'permissions',
      ok: permissions.microphone === 'granted',
      text:
        permissions.microphone === 'granted'
          ? t('onboarding-check-allowed')
          : t('onboarding-check-not-allowed'),
    },
    {
      id: 'system',
      step: 'permissions',
      ok: permissions.systemAudio === 'granted',
      text:
        permissions.systemAudio === 'granted'
          ? t('onboarding-check-allowed')
          : permissions.systemAudio === null
            ? t('onboarding-check-not-checked')
            : t('onboarding-check-not-allowed'),
    },
    {
      id: 'provider',
      step: 'provider',
      ok: summaryChosen,
      text: summaryChosen
        ? t('onboarding-check-provider-chosen')
        : t('onboarding-check-no-provider'),
    },
    { id: 'model', step: 'model', ...modelValue },
    {
      id: 'shortcut',
      step: 'everyday',
      ok: true,
      text: formatShortcut(app.globalShortcut, t),
    },
  ];
  const missing = rows.some((row) => !row.ok && !row.busy);
  return (
    <>
      <ul className="set-card onb-checks" aria-label={t('onboarding-checks')}>
        {rows.map((row) => (
          <li key={row.id} className="onb-check" data-ok={row.ok || undefined}>
            <span className="onb-check-mark" aria-hidden>
              {row.ok ? <CheckIcon weight="bold" /> : <span />}
            </span>
            <span className="onb-check-label">{t(`onboarding-check-${row.id}`)}</span>
            <span className="onb-check-value" data-mono={row.id === 'shortcut' || undefined}>
              {row.text}
            </span>
            {!row.ok && !row.busy && (
              <Button className="set-link" onPress={() => onGo(row.step)}>
                {t('onboarding-set-up')}
              </Button>
            )}
          </li>
        ))}
      </ul>
      {missing && <Note>{t('onboarding-ready-later')}</Note>}
    </>
  );
}
