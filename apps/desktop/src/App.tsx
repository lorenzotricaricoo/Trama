import { AnimatePresence, MotionConfig, motion, type Variants } from 'motion/react';
import { useLayoutEffect } from 'react';
import { useI18n } from './i18n/I18nProvider';
import { useJobs } from './jobs/useJobs';
import { duration, ease } from './motion';
import { Screen } from './screens/screens';
import { CommandBar, useCommandBar, type Command } from './shell/CommandBar';
import { NAV_ITEMS, navItemOf, useNavigation, type Direction } from './shell/navigation';
import { useFlushBeforeQuit } from './notes/flush';
import { Notice, useNotice } from './shell/Notice';
import { SHORTCUTS, useShortcuts, type ShortcutId } from './shell/shortcuts';
import { Sidebar } from './shell/Sidebar';
import { TopBar } from './shell/TopBar';
import { useSidebarCollapsed } from './shell/useSidebarCollapsed';
import { useTranslucency } from './shell/useTranslucency';
import { ArrowBendUpLeftIcon, NotePencilIcon, RecordIcon, SidebarSimpleIcon } from './ui/icons';
import { useColorScheme } from './useColorScheme';

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
  const scheme = useColorScheme();
  const translucent = useTranslucency();
  const nav = useNavigation();
  const [collapsed, toggleSidebar] = useSidebarCollapsed();
  const bar = useCommandBar();
  const jobs = useJobs();
  const notice = useNotice();
  // A note that could not be saved keeps the app open, with a way out.
  useFlushBeforeQuit(() => notice.show(t('quit-unsaved')));

  // The theme classes go on <html>, so that overlays rendered in portals get the tokens too.
  useLayoutEffect(() => {
    const root = document.documentElement;
    const classes = ['t-app', `t-${scheme}`, translucent ? 't-translucent' : 't-opaque'];
    root.classList.add(...classes);
    return () => root.classList.remove(...classes);
  }, [scheme, translucent]);

  // Recording arrives with the recording window (#31); until then the button explains it.
  const record = () => notice.show(t('record-unavailable'));
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
      label: t('record-meeting'),
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
  const title = nav.screen === 'note' ? t('note-title') : current ? t(current.label) : '';

  return (
    <MotionConfig reducedMotion="user">
      <div className="app" data-collapsed={collapsed || undefined}>
        {/* The content extends under the transparent title bar; this strip moves the window. */}
        <div className="titlebar" data-tauri-drag-region />
        <Sidebar
          active={navItemOf(nav.screen)}
          collapsed={collapsed}
          onNavigate={nav.navigate}
          onToggle={toggleSidebar}
          onRecord={record}
          onCommandBar={bar.open}
        />
        <main className="sheet">
          <TopBar title={title} canGoBack={nav.canGoBack} onBack={nav.back} jobs={jobs} />
          <div className="stage">
            <AnimatePresence mode="wait" initial={false} custom={nav.direction}>
              <motion.div
                // Every note gets its own transition, and its own editor.
                key={nav.screen === 'note' ? `note-${nav.entry}` : nav.screen}
                className="stage-screen"
                custom={nav.direction}
                variants={stage}
                initial="enter"
                animate="center"
                exit="exit"
              >
                <Screen id={nav.screen} nav={nav} onNotice={notice.show} />
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
