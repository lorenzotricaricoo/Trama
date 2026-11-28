import Markdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { openLink } from './api';

interface NotePreviewProps {
  text: string;
  /** Shown when the note has no text yet. */
  empty: string;
  /** Called when a link can't be opened. */
  onLinkFailed: () => void;
}

const plugins = [remarkGfm];

/**
 * The note as it reads: Markdown with tables, task lists and strikethrough (GitHub
 * flavour). Raw HTML is dropped, links open in the default browser instead of in the
 * window, and images show their description: a note never loads anything from the
 * network by itself.
 */
export function NotePreview({ text, empty, onLinkFailed }: NotePreviewProps) {
  if (!text.trim()) return <p className="note-preview-empty">{empty}</p>;

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
      <Markdown remarkPlugins={plugins} skipHtml components={components}>
        {text}
      </Markdown>
    </div>
  );
}
