import { motion, type Variants } from 'motion/react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Tab, TabList, TabPanel, Tabs } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import {
  DatabaseIcon,
  MicrophoneIcon,
  SlidersHorizontalIcon,
  SparkleIcon,
  WarningIcon,
  WaveformIcon,
  type Icon,
} from '../ui/icons';
import { api, errorCode, type AppSettings, type LibrarySettings, type StartupNotice } from './api';
import { DataSection } from './DataSection';
import { GeneralSection } from './GeneralSection';
import { ProvidersSection } from './ProvidersSection';
import { RecordingSection } from './RecordingSection';
import './settings.css';
import { TranscriptionSection } from './TranscriptionSection';
import { ErrorText, useNoticeText } from './ui';

type SectionId = 'general' | 'providers' | 'transcription' | 'recording' | 'data' | 'claude';

interface SectionDef {
  id: SectionId;
  label: string;
  icon: Icon;
  /** MVP phase that ships the section; later ones stay hidden. */
  phase: 1 | 2 | 3;
}

/** The phase this build ships; the Claude (MCP) section arrives with phase 3. */
const CURRENT_PHASE = 1;

const SECTIONS: readonly SectionDef[] = [
  { id: 'general', label: 'settings-section-general', icon: SlidersHorizontalIcon, phase: 1 },
  { id: 'providers', label: 'settings-section-providers', icon: SparkleIcon, phase: 1 },
  { id: 'transcription', label: 'settings-section-transcription', icon: WaveformIcon, phase: 1 },
  { id: 'recording', label: 'settings-section-recording', icon: MicrophoneIcon, phase: 1 },
  { id: 'data', label: 'settings-section-data', icon: DatabaseIcon, phase: 1 },
  { id: 'claude', label: 'settings-section-claude', icon: SparkleIcon, phase: 3 },
];

export const VISIBLE_SECTIONS = SECTIONS.filter((section) => section.phase <= CURRENT_PHASE);

const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** Saves a change of the library settings, showing it at once and undoing it on failure. */
export type UpdateLibrary = (change: Partial<LibrarySettings>) => Promise<string | null>;

/** Settings: providers and models, transcription, recording, data and privacy, general. */
export function SettingsScreen() {
  const { t } = useI18n();
  const noticeText = useNoticeText();
  const [app, setApp] = useState<AppSettings | null>(null);
  const [library, setLibrary] = useState<LibrarySettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notices, setNotices] = useState<StartupNotice[]>([]);
  // Number of the latest library change: answers to older ones are ignored.
  const libraryRequest = useRef(0);

  useEffect(() => {
    let active = true;
    // Notices are shown even when the library can't be read.
    void api
      .startupNotices()
      .then((list) => {
        if (active) setNotices(list);
      })
      .catch(() => {});
    Promise.all([api.appSettings(), api.librarySettings()])
      .then(([appSettings, librarySettings]) => {
        if (!active) return;
        setApp(appSettings);
        setLibrary(librarySettings);
      })
      .catch((error: unknown) => {
        if (active) setLoadError(errorCode(error));
      });
    return () => {
      active = false;
    };
  }, []);

  const updateLibrary: UpdateLibrary = useCallback(
    async (change) => {
      if (!library) return null;
      const request = ++libraryRequest.current;
      const previous = library;
      const next = { ...library, ...change };
      setLibrary(next);
      try {
        const saved = await api.setLibrarySettings(next);
        if (request === libraryRequest.current) setLibrary(saved);
        return null;
      } catch (error) {
        if (request === libraryRequest.current) setLibrary(previous);
        return errorCode(error);
      }
    },
    [library],
  );

  return (
    <motion.div className="screen set-screen" initial="hide" animate="show">
      <motion.header className="screen-top" variants={rise}>
        <h1 className="screen-title">{t('nav-settings')}</h1>
        <p className="screen-desc">{t('settings-description')}</p>
      </motion.header>
      {notices.length > 0 && (
        <div className="set-notices">
          {notices.map((notice) => (
            <p key={notice.code} className="set-warn" role="status">
              <WarningIcon aria-hidden />
              {noticeText(notice)}
            </p>
          ))}
        </div>
      )}
      {loadError !== null ? (
        <div className="set-load">
          <ErrorText code={loadError} />
        </div>
      ) : app === null || library === null ? (
        <p className="set-load" role="status">
          {t('settings-loading')}
        </p>
      ) : (
        <motion.div variants={rise} className="set-body">
          <Tabs orientation="vertical" className="set-tabs">
            <TabList aria-label={t('settings-sections')} className="set-tablist">
              {VISIBLE_SECTIONS.map(({ id, label, icon: SectionIcon }) => (
                <Tab key={id} id={id} className="set-tab">
                  <SectionIcon aria-hidden className="set-tab-ic" />
                  <span>{t(label)}</span>
                </Tab>
              ))}
            </TabList>
            <TabPanel id="general" className="set-panel">
              <GeneralSection
                app={app}
                onApp={setApp}
                library={library}
                onLibrary={updateLibrary}
              />
            </TabPanel>
            <TabPanel id="providers" className="set-panel">
              <ProvidersSection library={library} onLibrary={updateLibrary} />
            </TabPanel>
            <TabPanel id="transcription" className="set-panel">
              <TranscriptionSection />
            </TabPanel>
            <TabPanel id="recording" className="set-panel">
              <RecordingSection app={app} onApp={setApp} />
            </TabPanel>
            <TabPanel id="data" className="set-panel">
              <DataSection app={app} onApp={setApp} library={library} onLibrary={updateLibrary} />
            </TabPanel>
          </Tabs>
        </motion.div>
      )}
    </motion.div>
  );
}
