import { motion } from 'motion/react';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { spring } from '../motion';

/**
 * Pill in the top bar while the core works in the background: what it is doing and how
 * far it got. Failed jobs are shown where their source is listed, not here.
 */
export function JobIndicator({ jobs }: { jobs: readonly Job[] }) {
  const { t, locale } = useI18n();
  const active = jobs.filter((job) => job.state === 'running' || job.state === 'queued');
  const [first] = [...active].sort(
    (a, b) => Number(b.state === 'running') - Number(a.state === 'running'),
  );
  if (!first) return null;

  const label = t(first.state === 'running' ? `job-${first.kind}` : 'job-queued');
  const percent =
    first.progress === null
      ? null
      : new Intl.NumberFormat(locale, { style: 'percent' }).format(first.progress);
  const more = active.length - 1;

  return (
    <motion.div
      className="job"
      role="status"
      initial={{ opacity: 0, scale: 0.5 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={spring.pop}
    >
      <span className="job-pulse" aria-hidden />
      <span>{label}</span>
      {percent !== null && (
        <>
          <span className="bar bar-live" aria-hidden>
            <span className="bar-fill" style={{ transform: `scaleX(${first.progress})` }} />
          </span>
          <span className="job-pct">{percent}</span>
        </>
      )}
      {more > 0 && <span className="kbd">{t('job-more', { count: more })}</span>}
    </motion.div>
  );
}
