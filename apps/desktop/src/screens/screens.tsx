import { lazy, Suspense, useMemo } from 'react';
import { useI18n } from '../i18n/I18nProvider';
import type { Navigation, ScreenId } from '../shell/navigation';
import { SettingsScreen } from '../settings/SettingsScreen';
import { HouseIcon } from '../ui/icons';
import { PlaceholderScreen } from './PlaceholderScreen';
import { SourcesScreen } from './SourcesScreen';

// Home and the full Sources list arrive with #35.

// The editor (CodeMirror) loads with the first note, not with the app.
const NoteScreen = lazy(() => import('../notes/NoteScreen'));

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

export function Screen({
  id,
  nav,
  onNotice,
}: {
  id: ScreenId;
  nav: Navigation;
  onNotice: (message: string) => void;
}) {
  switch (id) {
    case 'home':
      return <HomeScreen />;
    case 'sources':
      return (
        <SourcesScreen
          onOpenNote={(noteId) => nav.navigate('note', noteId)}
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
  }
}
