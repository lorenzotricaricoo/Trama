import { lazy, Suspense, useMemo } from 'react';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import type { Navigation, ScreenId } from '../shell/navigation';
import { GearIcon, HouseIcon } from '../ui/icons';
import { PlaceholderScreen } from './PlaceholderScreen';
import { SourcesScreen } from './SourcesScreen';

// Home and the full Sources list arrive with #35, Settings with #36.

// The editor (CodeMirror) loads with the first note, not with the app.
const NoteScreen = lazy(() => import('../notes/NoteScreen'));
const MeetingScreen = lazy(() => import('../meetings/MeetingScreen'));

function HomeScreen() {
  const { t, locale } = useI18n();
  const today = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long' }).format(
        new Date(),
      ),
    [locale],
  );
  return (
    <PlaceholderScreen
      kicker={today}
      title={t('welcome-title')}
      description={t('home-description')}
      icon={HouseIcon}
      emptyTitle={t('home-empty-title')}
      emptyBody={t('home-empty-body')}
    />
  );
}

function SettingsScreen() {
  const { t } = useI18n();
  return (
    <PlaceholderScreen
      title={t('nav-settings')}
      description={t('settings-description')}
      icon={GearIcon}
      emptyTitle={t('settings-empty-title')}
      emptyBody={t('settings-empty-body')}
    />
  );
}

export function Screen({
  id,
  nav,
  jobs,
  onNotice,
}: {
  id: ScreenId;
  nav: Navigation;
  jobs: readonly Job[];
  onNotice: (message: string) => void;
}) {
  switch (id) {
    case 'home':
      return <HomeScreen />;
    case 'sources':
      return (
        <SourcesScreen
          onOpenNote={(noteId) => nav.navigate('note', noteId)}
          onOpenMeeting={(meetingId) => nav.navigate('meeting', meetingId)}
          onNewNote={() => nav.navigate('note')}
        />
      );
    case 'settings':
      return <SettingsScreen />;
    case 'note':
      return (
        <Suspense fallback={null}>
          <NoteScreen
            noteId={nav.noteId}
            entry={nav.entry}
            onCreated={nav.setNoteId}
            onNotice={onNotice}
          />
        </Suspense>
      );
    case 'meeting':
      return nav.meetingId === undefined ? null : (
        <Suspense fallback={null}>
          <MeetingScreen meetingId={nav.meetingId} jobs={jobs} onNotice={onNotice} />
        </Suspense>
      );
  }
}
