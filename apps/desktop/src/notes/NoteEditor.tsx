import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { markdownKeymap, markdownLanguage } from '@codemirror/lang-markdown';
import { HighlightStyle, LanguageSupport, syntaxHighlighting } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, placeholder as placeholderText } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { useEffect, useRef } from 'react';

interface NoteEditorProps {
  /** Text when the editor opens; later changes come from the user. */
  initialText: string;
  /** Accessible name of the text area. */
  label: string;
  placeholder: string;
  onChange: (text: string) => void;
  onBlur: () => void;
}

/**
 * Markdown styled in place, as in a writing app: the marks stay visible (and are saved
 * exactly as typed), quieter than the text they format. Colours come from the tokens.
 */
const markdownStyle = HighlightStyle.define([
  { tag: tags.heading1, fontSize: '1.6em', fontWeight: '650', letterSpacing: '-0.02em' },
  { tag: tags.heading2, fontSize: '1.3em', fontWeight: '650' },
  { tag: [tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontWeight: '650' },
  { tag: tags.strong, fontWeight: '650' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strikethrough, textDecoration: 'line-through' },
  { tag: [tags.link, tags.url], color: 'var(--acc-t)' },
  { tag: tags.monospace, fontFamily: 'var(--font-mono)', fontSize: '0.92em' },
  { tag: tags.quote, color: 'var(--mute)' },
  { tag: [tags.processingInstruction, tags.meta, tags.contentSeparator], color: 'var(--faint)' },
]);

const theme = EditorView.theme({
  '&': { height: '100%', color: 'var(--ink)', backgroundColor: 'transparent' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font)', lineHeight: '1.6', overflow: 'visible' },
  '.cm-content': {
    padding: '0',
    caretColor: 'var(--ink)',
    // The app turns text selection off outside content.
    userSelect: 'text',
    WebkitUserSelect: 'text',
    cursor: 'text',
  },
  '.cm-line': { padding: '0' },
  '.cm-placeholder': { color: 'var(--faint)' },
  '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--ink)' },
});

/**
 * The note editor: CodeMirror 6 with the Markdown language. Native selection and input are
 * kept (no custom caret), so IME and accent composition, spelling and VoiceOver behave as in
 * any macOS text area. Tab is left to focus navigation.
 */
export function NoteEditor(props: NoteEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  // The latest props, read by the editor's listeners; the editor itself is created once.
  const latest = useRef(props);
  useEffect(() => {
    latest.current = props;
  });

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const { initialText, label, placeholder } = latest.current;
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: initialText,
        extensions: [
          history(),
          EditorView.lineWrapping,
          new LanguageSupport(markdownLanguage),
          syntaxHighlighting(markdownStyle),
          keymap.of([...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
          placeholderText(placeholder),
          theme,
          EditorView.contentAttributes.of({
            'aria-label': label,
            spellcheck: 'true',
            autocorrect: 'on',
            autocapitalize: 'sentences',
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) latest.current.onChange(update.state.doc.toString());
          }),
          EditorView.domEventHandlers({
            blur: () => {
              latest.current.onBlur();
              return false;
            },
          }),
        ],
      }),
    });
    // At the end of the text, ready to type.
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    view.focus();
    return () => {
      view.destroy();
    };
  }, []);

  return <div ref={host} className="note-editor" />;
}
