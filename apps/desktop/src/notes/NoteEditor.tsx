import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { markdownKeymap, markdownLanguage } from '@codemirror/lang-markdown';
import { HighlightStyle, LanguageSupport, syntaxHighlighting } from '@codemirror/language';
import { EditorSelection, EditorState, type ChangeSpec } from '@codemirror/state';
import { EditorView, keymap, placeholder as placeholderText } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { useEffect, useImperativeHandle, useRef, type Ref } from 'react';

/** Formatting the toolbar and the shortcuts apply to the selection. */
export type FormatAction =
  'heading' | 'bold' | 'italic' | 'bullets' | 'numbers' | 'todo' | 'quote' | 'code' | 'link';

export interface NoteEditorHandle {
  format: (action: FormatAction) => void;
  focus: () => void;
}

interface NoteEditorProps {
  /** Text when the editor opens; later changes come from the user. */
  initialText: string;
  /** Accessible name of the text area. */
  label: string;
  placeholder: string;
  /** Puts the caret at the end of the text when the editor opens. */
  autoFocus: boolean;
  onChange: (text: string) => void;
  onBlur: () => void;
  ref?: Ref<NoteEditorHandle>;
}

/**
 * Markdown styled in place, as in a writing app: the marks stay visible (and are saved
 * exactly as typed), quieter than the text they format. Colours come from the tokens.
 */
const markdownStyle = HighlightStyle.define([
  { tag: tags.heading1, fontSize: '1.5em', fontWeight: '650', letterSpacing: '-0.02em' },
  { tag: tags.heading2, fontSize: '1.25em', fontWeight: '650' },
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
  '.cm-scroller': { fontFamily: 'var(--font)', lineHeight: '1.65', overflow: 'auto' },
  '.cm-content': {
    // The whole page is the document: the text sits in a readable column, and the
    // empty space below it still places the caret.
    maxWidth: '720px',
    margin: '0 auto',
    padding: '4px 0 40vh',
    caretColor: 'var(--ink)',
    // The app turns text selection off outside content.
    userSelect: 'text',
    WebkitUserSelect: 'text',
    cursor: 'text',
  },
  '.cm-line': { padding: '0 2px' },
  '.cm-placeholder': { color: 'var(--faint)' },
  '&.cm-focused .cm-cursor': { borderLeftColor: 'var(--ink)' },
});

const INLINE: Partial<Record<FormatAction, string>> = { bold: '**', italic: '*', code: '`' };
const LINE: Partial<Record<FormatAction, string>> = {
  heading: '## ',
  bullets: '- ',
  numbers: '1. ',
  todo: '- [ ] ',
  quote: '> ',
};
/** Marks that open a line; a new line format replaces the one already there. */
const LINE_MARK = /^(#{1,6} |> |- \[[ xX]\] |[-*+] |\d+[.)] )/;

/** Wraps each selection in `mark`, or unwraps it when it is already wrapped. */
function toggleInline(view: EditorView, mark: string) {
  const { state } = view;
  view.dispatch(
    state.changeByRange((range) => {
      const before = state.sliceDoc(range.from - mark.length, range.from);
      const after = state.sliceDoc(range.to, range.to + mark.length);
      if (before === mark && after === mark) {
        return {
          changes: [
            { from: range.from - mark.length, to: range.from },
            { from: range.to, to: range.to + mark.length },
          ],
          range: EditorSelection.range(range.from - mark.length, range.to - mark.length),
        };
      }
      return {
        changes: [
          { from: range.from, insert: mark },
          { from: range.to, insert: mark },
        ],
        range: EditorSelection.range(range.from + mark.length, range.to + mark.length),
      };
    }),
  );
}

/** Starts every selected line with `mark`, or removes it when all of them have it. */
function toggleLine(view: EditorView, mark: string) {
  const { state } = view;
  const lines = new Map<number, string>();
  for (const range of state.selection.ranges) {
    for (let n = state.doc.lineAt(range.from).number; n <= state.doc.lineAt(range.to).number; n++) {
      const line = state.doc.line(n);
      lines.set(line.from, line.text);
    }
  }
  const all = [...lines.values()].every((text) => text.startsWith(mark));
  const changes: ChangeSpec[] = [...lines].map(([from, text]) => {
    const existing = LINE_MARK.exec(text)?.[0] ?? '';
    return all
      ? { from, to: from + mark.length }
      : { from, to: from + existing.length, insert: mark };
  });
  view.dispatch({ changes });
}

/** Turns the selection into a link and selects the address, ready to paste over it. */
function insertLink(view: EditorView) {
  const { state } = view;
  const url = 'https://';
  view.dispatch(
    state.changeByRange((range) => {
      const text = state.sliceDoc(range.from, range.to);
      const insert = `[${text}](${url})`;
      const urlStart = range.from + text.length + 3;
      return {
        changes: { from: range.from, to: range.to, insert },
        range: text
          ? EditorSelection.range(urlStart, urlStart + url.length)
          : EditorSelection.cursor(range.from + 1),
      };
    }),
  );
}

function applyFormat(view: EditorView, action: FormatAction) {
  const inline = INLINE[action];
  const line = LINE[action];
  if (inline) toggleInline(view, inline);
  else if (line) toggleLine(view, line);
  else insertLink(view);
  view.focus();
}

/**
 * The note editor: CodeMirror 6 with the Markdown language, filling its pane like a
 * document. Native selection and input are kept (no custom caret), so IME and accent
 * composition, spelling and VoiceOver behave as in any macOS text area. Tab is left to
 * focus navigation.
 */
export function NoteEditor({ ref, ...props }: NoteEditorProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // The latest props, read by the editor's listeners; the editor itself is created once.
  const latest = useRef(props);
  useEffect(() => {
    latest.current = props;
  });

  useImperativeHandle(ref, () => ({
    format: (action) => {
      if (view.current) applyFormat(view.current, action);
    },
    focus: () => view.current?.focus(),
  }));

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const { initialText, label, placeholder, autoFocus } = latest.current;
    const editor = new EditorView({
      parent,
      state: EditorState.create({
        doc: initialText,
        extensions: [
          history(),
          EditorView.lineWrapping,
          new LanguageSupport(markdownLanguage),
          syntaxHighlighting(markdownStyle),
          keymap.of([
            { key: 'Mod-b', run: (v) => (applyFormat(v, 'bold'), true) },
            { key: 'Mod-i', run: (v) => (applyFormat(v, 'italic'), true) },
            ...markdownKeymap,
            ...defaultKeymap,
            ...historyKeymap,
          ]),
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
    view.current = editor;
    if (autoFocus) {
      // At the end of the text, ready to type.
      editor.dispatch({ selection: { anchor: editor.state.doc.length } });
      editor.focus();
    }
    return () => {
      view.current = null;
      editor.destroy();
    };
  }, []);

  return <div ref={host} className="note-editor" />;
}
