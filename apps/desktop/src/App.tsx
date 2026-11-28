import { AnimatePresence, MotionConfig, motion, type Variants } from 'motion/react';
import { useState } from 'react';
import { useI18n } from './i18n/I18nProvider';
import { useJobs } from './jobs/useJobs';
import { duration, ease } from './motion';
import { Screen } from './screens/screens';
import { useStartupNotices } from './settings/useStartupNotices';
import { CommandBar, useCommandBar, type Command } from './shell/CommandBar';
import { NAV_ITEMS, navItemOf, useNavigation, type Direction } from './shell/navigation';
import { useFlushBeforeQuit } from './notes/flush';
import { showRecordingPanel, startRecording, stopRecording } from './recording/api';
import { errorCode, recordingErrorMessage } from './recording/errors';
import { formatClock } from './recording/format';
import { useElapsed, useRecordingState } from './recording/useRecordingState';
import { Notice, useNotice } from './shell/Notice';
import { SHORTCUTS, useShortcuts, type ShortcutId } from './shell/shortcuts';
import { Sidebar } from './shell/Sidebar';
import { TopBar } from './shell/TopBar';
import { useSidebarCollapsed } from './shell/useSidebarCollapsed';
import { ArrowBendUpLeftIcon, NotePencilIcon, RecordIcon, SidebarSimpleIcon } from './ui/icons';
import { useAppearance } from './appearance';
import { useThemeClasses } from './useThemeClasses';

const COMMAND_BAR_SHORTCUTS: readonly ShortcutId[] = ['commandBar', 'settings', 'newNote'];

// Screens leave quickly towards where they came from and enter from the direction of travel.
const stage: Variants = {
  enter: (direction: Direction) => ({ opacity: 0, y: direction === 'down' ? 14 : -14 }),
  center: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
  exit: (direction: Direction) => ({
    opacity: 0,
    y: direction === 'down' ? -8 : 8,
    transition: { duration: duration.d1, ease: ease.in },
  }),
};

export function App() {
  const { t } = useI18n();
  useThemeClasses();
  const { reduceMotion } = useAppearance();
  const nav = useNavigation();
  const [collapsed, toggleSidebar] = useSidebarCollapsed();
  const bar = useCommandBar();
  const jobs = useJobs();
  const notice = useNotice();
  useStartupNotices(notice.show);
  // A note that could not be saved keeps the app open, with a way out.
  useFlushBeforeQuit(() => notice.show(t('quit-unsaved')));

  // Flow F2: record, stop (here or in the panel), then the meeting page follows the
  // transcription and the summary.
  const recording = useRecordingState((meetingId) => nav.navigate('meeting', meetingId));
  const elapsed = useElapsed(recording.status, recording.receivedAt);
  const [busy, setBusy] = useState(false);
  const failed = (error: unknown) => {
    const code = errorCode(error);
    notice.show(t(recordingErrorMessage(code), { code }));
  };
  const record = () => {
    if (busy) return;
    setBusy(true);
    const action = recording.status
      ? stopRecording().then(() => undefined)
      : startRecording().then(recording.set);
    action.catch(failed).finally(() => setBusy(false));
  };
  const showPanel = () => {
    showRecordingPanel().catch((error: unknown) => {
      console.error('Cannot show the recording panel', error);
    });
  };
  const recordLabel = recording.status ? t('record-stop-meeting') : t('record-meeting');
  const sidebarLabel = collapsed ? t('sidebar-expand') : t('sidebar-collapse');
  // Flow F3: ⌘N opens an empty editor; the note exists once its first text is saved.
  const newNote = () => {
    bar.close();
    nav.navigate('note');
  };

  useShortcuts(
    {
      commandBar: bar.toggle,
      toggleSidebar,
      settings: () => {
        bar.close();
        nav.navigate('settings');
      },
      back: nav.back,
      record,
      newNote,
    },
    // Behind the command bar only its own toggle and Settings act.
    bar.state === 'closed' ? undefined : COMMAND_BAR_SHORTCUTS,
  );

  const commands: Command[] = [
    ...NAV_ITEMS.map((item): Command => ({
      id: `go-${item.id}`,
      group: 'navigate',
      label: t(item.label),
      icon: item.icon,
      shortcut: item.id === 'settings' ? SHORTCUTS.settings.label : undefined,
      run: () => nav.navigate(item.id),
    })),
    {
      id: 'new-note',
      group: 'actions',
      label: t('note-new'),
      icon: NotePencilIcon,
      shortcut: SHORTCUTS.newNote.label,
      run: newNote,
    },
    {
      id: 'record',
      group: 'actions',
      label: recordLabel,
      icon: RecordIcon,
      shortcut: SHORTCUTS.record.label,
      run: record,
    },
    {
      id: 'toggle-sidebar',
      group: 'actions',
      label: sidebarLabel,
      icon: SidebarSimpleIcon,
      shortcut: SHORTCUTS.toggleSidebar.label,
      run: toggleSidebar,
    },
  ];
  if (nav.canGoBack) {
    commands.push({
      id: 'back',
      group: 'actions',
      label: t('back'),
      icon: ArrowBendUpLeftIcon,
      shortcut: SHORTCUTS.back.label,
      run: nav.back,
    });
  }

  const current = NAV_ITEMS.find((item) => item.id === nav.screen);
  const title =
    nav.screen === 'note'
      ? t('note-title')
      : nav.screen === 'meeting'
        ? t('meeting-title')
        : current
          ? t(current.label)
          : '';

  return (
    <MotionConfig reducedMotion={reduceMotion ? 'always' : 'user'}>
      <div className="app" data-collapsed={collapsed || undefined}>
        {/* The content extends under the transparent title bar; this strip moves the window. */}
        <div className="titlebar" data-tauri-drag-region />
        <Sidebar
          active={navItemOf(nav.screen)}
          collapsed={collapsed}
          onNavigate={nav.navigate}
          onToggle={toggleSidebar}
          onRecord={record}
          recording={
            recording.status
              ? { label: t('record-in-progress-short', { clock: formatClock(elapsed) }) }
              : undefined
          }
          onCommandBar={bar.open}
        />
        <main className="sheet">
          <TopBar
            title={title}
            canGoBack={nav.canGoBack}
            onBack={nav.back}
            jobs={jobs}
            recordingClock={recording.status ? formatClock(elapsed) : undefined}
            onShowRecording={showPanel}
          />
          <div className="stage">
            <AnimatePresence mode="wait" initial={false} custom={nav.direction}>
              <motion.div
                // Every note gets its own transition, and its own editor.
                key={
                  nav.screen === 'note' || nav.screen === 'meeting'
                    ? `${nav.screen}-${nav.entry}`
                    : nav.screen
                }
                className="stage-screen"
                custom={nav.direction}
                variants={stage}
                initial="enter"
                animate="center"
                exit="exit"
              >
                <Screen id={nav.screen} nav={nav} jobs={jobs} onNotice={notice.show} />
              </motion.div>
            </AnimatePresence>
          </div>
          <Notice notice={notice.notice} onDismiss={notice.dismiss} />
        </main>
        <CommandBar bar={bar} commands={commands} />
      </div>
    </MotionConfig>
  );
}
