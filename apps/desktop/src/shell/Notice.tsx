import { AnimatePresence, motion } from 'motion/react';
import { useCallback, useEffect, useState } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease, spring } from '../motion';
import { InfoIcon, XIcon } from '../ui/icons';

interface NoticeValue {
  id: number;
  text: string;
}

const VISIBLE_MS = 4200;
/** Longer messages (an import listing skipped files) stay longer, up to this. */
const MAX_VISIBLE_MS = 12_000;
/** Extra time per character beyond the first 80, about a reading pace. */
const MS_PER_CHAR = 45;

/** How long a message stays on screen. */
export function visibleMs(text: string): number {
  return Math.min(MAX_VISIBLE_MS, VISIBLE_MS + Math.max(0, text.length - 80) * MS_PER_CHAR);
}

/** A short message at the bottom of the sheet that goes away by itself. */
export function useNotice() {
  const [notice, setNotice] = useState<NoticeValue | null>(null);
  const show = useCallback((text: string) => {
    setNotice((current) => ({ id: (current?.id ?? 0) + 1, text }));
  }, []);
  const dismiss = useCallback(() => setNotice(null), []);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), visibleMs(notice.text));
    return () => clearTimeout(timer);
  }, [notice]);

  return { notice, show, dismiss };
}

export function Notice({
  notice,
  onDismiss,
}: {
  notice: NoticeValue | null;
  onDismiss: () => void;
}) {
  const { t } = useI18n();
  return (
    // Always in the page, so screen readers announce what appears in it.
    <div className="notices" role="status">
      <AnimatePresence>
        {notice && (
          <motion.div
            key={notice.id}
            className="notice"
            initial={{ opacity: 0, y: 18, scale: 0.94 }}
            animate={{ opacity: 1, y: 0, scale: 1, transition: spring.pop }}
            exit={{
              opacity: 0,
              y: 8,
              scale: 0.96,
              transition: { duration: duration.d2, ease: ease.in },
            }}
          >
            <InfoIcon className="notice-ic" aria-hidden />
            <span className="notice-text">{notice.text}</span>
            <Button className="icon-btn" aria-label={t('notice-close')} onPress={onDismiss}>
              <XIcon aria-hidden />
            </Button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
