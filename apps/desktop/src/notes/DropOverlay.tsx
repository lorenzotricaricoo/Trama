import { AnimatePresence, motion } from 'motion/react';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease } from '../motion';
import { SHORTCUTS } from '../shell/shortcuts';
import { DownloadSimpleIcon } from '../ui/icons';

/**
 * Shown over the sheet while Markdown files are held over the window: dropping them
 * imports each one as a note.
 */
export function DropOverlay({ visible }: { visible: boolean }) {
  const { t } = useI18n();
  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          className="drop"
          role="status"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1, transition: { duration: duration.d2, ease: ease.out } }}
          exit={{ opacity: 0, transition: { duration: duration.d1, ease: ease.in } }}
        >
          <motion.div
            className="drop-zone"
            initial={{ scale: 0.98 }}
            animate={{ scale: 1, transition: { duration: duration.d3, ease: ease.out } }}
          >
            <span className="drop-ic" aria-hidden>
              <DownloadSimpleIcon />
            </span>
            <p className="drop-title">{t('import-drop-title')}</p>
            <p className="drop-body">{t('import-drop-body')}</p>
            <p className="drop-hint">
              {t('import-notes')} <kbd className="kbd">{SHORTCUTS.importNotes.label}</kbd>
            </p>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
