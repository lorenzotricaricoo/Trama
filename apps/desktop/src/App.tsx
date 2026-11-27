import { MotionConfig, motion } from 'motion/react';
import { useMemo } from 'react';
import { useI18n } from './i18n/I18nProvider';
import { duration, ease } from './motion';
import { useColorScheme } from './useColorScheme';

export function App() {
  const { t, locale } = useI18n();
  const scheme = useColorScheme();
  const today = useMemo(
    () => new Intl.DateTimeFormat(locale, { dateStyle: 'full' }).format(new Date()),
    [locale],
  );

  return (
    <MotionConfig reducedMotion="user">
      <main className={`t-app t-${scheme} app`}>
        {/* The content extends under the transparent title bar; this strip moves the window. */}
        <div className="titlebar" data-tauri-drag-region />
        <motion.section
          className="welcome"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: duration.d4, ease: ease.out }}
        >
          <h1>{t('welcome-title')}</h1>
          <p>{t('welcome-body')}</p>
          <p className="muted">{t('today', { date: today })}</p>
          <p className="faint">{t('welcome-status')}</p>
        </motion.section>
      </main>
    </MotionConfig>
  );
}
