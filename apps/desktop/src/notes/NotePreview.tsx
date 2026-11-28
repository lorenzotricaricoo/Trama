import Markdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useI18n } from '../i18n/I18nProvider';
import { openLink } from './api';

interface NotePreviewProps {
  text: string;
  /** Shown when the note has no text yet. */
  empty: string;
  /** Called when a link can't be opened. */
  onLinkFailed: () => void;
}

const plugins = [remarkGfm];

/** YAML front matter at the very start of a note, as other Markdown apps write it. */
const FRONT_MATTER = /^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/;

/**
 * Splits the front matter off a note's text. Markdown would read it as a rule and a
 * heading; the preview shows it as it is, apart from the text.
 */
export function splitFrontMatter(text: string): { frontMatter: string | null; body: string } {
  const match = FRONT_MATTER.exec(text);
  if (!match) return { frontMatter: null, body: text };
  return { frontMatter: match[1] ?? '', body: text.slice(match[0].length) };
}

/**
 * The note as it reads: Markdown with tables, task lists and strikethrough (GitHub
 * flavour). Raw HTML is dropped, links open in the default browser instead of in the
 * window, and images show their description: a note never loads anything from the
 * network by itself.
 */
export function NotePreview({ text, empty, onLinkFailed }: NotePreviewProps) {
  const { t } = useI18n();
  if (!text.trim()) return <p className="note-preview-empty">{empty}</p>;
  const { frontMatter, body } = splitFrontMatter(text);

  const components: Components = {
    a: ({ href, children }) => (
      <a
        href={href}
        title={href}
        onClick={(event) => {
          event.preventDefault();
          if (href) openLink(href).catch(onLinkFailed);
        }}
      >
        {children}
      </a>
    ),
    img: ({ alt }) => (alt ? <span className="note-preview-img">{alt}</span> : null),
  };

  return (
    <div className="note-preview">
      {frontMatter !== null && (
        <pre className="note-front-matter" aria-label={t('note-front-matter')}>
          {frontMatter}
        </pre>
      )}
      <Markdown remarkPlugins={plugins} skipHtml components={components}>
        {body}
      </Markdown>
    </div>
  );
}
