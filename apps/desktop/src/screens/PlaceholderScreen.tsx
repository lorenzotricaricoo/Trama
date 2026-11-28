import { motion, type Variants } from 'motion/react';
import { duration, ease } from '../motion';
import type { Icon } from '../ui/icons';

interface PlaceholderScreenProps {
  kicker?: string;
  title: string;
  description: string;
  icon: Icon;
  emptyTitle: string;
  emptyBody: string;
}

// Header and content enter one after the other.
const list: Variants = { show: { transition: { staggerChildren: 0.045, delayChildren: 0.03 } } };
const rise: Variants = {
  hide: { opacity: 0, y: 10 },
  show: { opacity: 1, y: 0, transition: { duration: duration.d4, ease: ease.out } },
};

/** A screen whose content arrives in a later issue: its title and an empty state. */
export function PlaceholderScreen({
  kicker,
  title,
  description,
  icon: EmptyIcon,
  emptyTitle,
  emptyBody,
}: PlaceholderScreenProps) {
  return (
    <motion.div className="screen" variants={list} initial="hide" animate="show">
      <motion.header className="screen-top" variants={rise}>
        {kicker && <div className="kicker">{kicker}</div>}
        <h1 className="screen-title">{title}</h1>
        <p className="screen-desc">{description}</p>
      </motion.header>
      <motion.section className="empty" variants={rise} aria-label={emptyTitle}>
        <span className="empty-ic" aria-hidden>
          <EmptyIcon />
        </span>
        <h2 className="empty-title">{emptyTitle}</h2>
        <p className="empty-body">{emptyBody}</p>
      </motion.section>
    </motion.div>
  );
}
