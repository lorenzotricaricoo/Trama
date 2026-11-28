import { lazy, Suspense } from 'react';
import type { Job } from '../jobs/useJobs';
import type { Navigation, ScreenId } from '../shell/navigation';
import { SettingsScreen } from '../settings/SettingsScreen';
import { HomeScreen } from './HomeScreen';
import { SourcesScreen } from './SourcesScreen';

// The editor (CodeMirror) loads with the first note, not with the app.
const NoteScreen = lazy(() => import('../notes/NoteScreen'));
const MeetingScreen = lazy(() => import('../meetings/MeetingScreen'));

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
      return (
        <HomeScreen
          jobs={jobs}
          onOpenNote={(noteId) => nav.navigate('note', noteId)}
          onOpenMeeting={(meetingId) => nav.navigate('meeting', meetingId)}
          onSources={() => nav.navigate('sources')}
          onNewNote={() => nav.navigate('note')}
        />
      );
    case 'sources':
      return (
        <SourcesScreen
          jobs={jobs}
          onOpenNote={(noteId) => nav.navigate('note', noteId)}
          onOpenMeeting={(meetingId, atMs) => nav.navigate('meeting', meetingId, atMs)}
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
          <MeetingScreen
            meetingId={nav.meetingId}
            startAtMs={nav.meetingAt}
            jobs={jobs}
            onNotice={onNotice}
          />
        </Suspense>
      );
  }
}
