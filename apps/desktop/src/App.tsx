import { AnimatePresence, MotionConfig, motion, type Variants } from 'motion/react';
import { useLayoutEffect } from 'react';
import { useI18n } from './i18n/I18nProvider';
import { useJobs } from './jobs/useJobs';
import { duration, ease } from './motion';
import { Screen } from './screens/screens';
import { CommandBar, useCommandBar, type Command } from './shell/CommandBar';
import { NAV_ITEMS, useNavigation, type Direction } from './shell/navigation';
import { Notice, useNotice } from './shell/Notice';
import { SHORTCUTS, useShortcuts } from './shell/shortcuts';
import { Sidebar } from './shell/Sidebar';
import { TopBar } from './shell/TopBar';
import { useSidebarCollapsed } from './shell/useSidebarCollapsed';
import { useTranslucency } from './shell/useTranslucency';
import { ArrowBendUpLeftIcon, RecordIcon, SidebarSimpleIcon } from './ui/icons';
import { useColorScheme } from './useColorScheme';

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

  useShortcuts({
    commandBar: bar.toggle,
    toggleSidebar,
    settings: () => {
      bar.close();
      nav.navigate('settings');
    },
    back: nav.back,
    record,
  });

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

  return (
    <MotionConfig reducedMotion="user">
      <div className="app" data-collapsed={collapsed || undefined}>
        {/* The content extends under the transparent title bar; this strip moves the window. */}
        <div className="titlebar" data-tauri-drag-region />
        <Sidebar
          active={nav.screen}
          collapsed={collapsed}
          onNavigate={nav.navigate}
          onToggle={toggleSidebar}
          onRecord={record}
          onCommandBar={bar.open}
        />
        <main className="sheet">
          <TopBar
            title={current ? t(current.label) : ''}
            canGoBack={nav.canGoBack}
            onBack={nav.back}
            jobs={jobs}
          />
          <div className="stage">
            <AnimatePresence mode="wait" initial={false} custom={nav.direction}>
              <motion.div
                key={nav.screen}
                className="stage-screen"
                custom={nav.direction}
                variants={stage}
                initial="enter"
                animate="center"
                exit="exit"
              >
                <Screen id={nav.screen} />
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
