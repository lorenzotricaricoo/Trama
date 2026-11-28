import { useState, type KeyboardEvent } from 'react';
import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { PencilSimpleIcon } from '../ui/icons';

interface MeetingTitleProps {
  /** The stored title; empty until a summary or the user gives one. */
  title: string;
  /** What shows while the title is empty. */
  placeholder: string;
  /** Saves the title; an empty one gives the title back to the summaries. */
  onRename: (title: string) => Promise<void>;
}

const tidy = (text: string) => text.split(/\s+/).filter(Boolean).join(' ');

/** The page title: a heading that turns into a field when pressed. */
export function MeetingTitle({ title, placeholder, onRename }: MeetingTitleProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<string | null>(null);

  const commit = () => {
    if (draft === null) return;
    const next = tidy(draft);
    setDraft(null);
    if (next !== title) void onRename(next);
  };

  const onKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      commit();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      setDraft(null);
    }
  };

  if (draft !== null) {
    return (
      <input
        className="screen-title mt-title-input"
        value={draft}
        placeholder={placeholder}
        aria-label={t('meeting-title-label')}
        maxLength={200}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={onKey}
        onBlur={commit}
        autoFocus
      />
    );
  }

  return (
    <h1 className="screen-title mt-title" data-untitled={!title || undefined}>
      <Button className="mt-title-btn" onPress={() => setDraft(title)}>
        {title || placeholder}
        <PencilSimpleIcon className="mt-title-pen" aria-hidden />
      </Button>
    </h1>
  );
}
