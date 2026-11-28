import { isTauri } from '@tauri-apps/api/core';
import { motion } from 'motion/react';
import { useCallback, useEffect, useState } from 'react';
import {
  Autocomplete,
  Dialog,
  Header,
  Input,
  Keyboard,
  Menu,
  MenuItem,
  MenuSection,
  Modal,
  ModalOverlay,
  TextField,
  useFilter,
  type Key,
} from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { duration, ease, spring } from '../motion';
import { searchSources, type SearchResult, type SourceRow } from '../notes/api';
import { formatClock } from '../recording/format';
import { Highlighted, useSourceFormat } from '../screens/sources';
import { MagnifyingGlassIcon, NoteIcon, WaveformIcon, type Icon } from '../ui/icons';

export interface Command {
  id: string;
  group: 'navigate' | 'actions';
  /** Translated label, also used to filter. */
  label: string;
  icon: Icon;
  shortcut?: string;
  run: () => void;
}

/** Open state of the command bar, kept open while its exit animation runs. */
type BarState = 'closed' | 'open' | 'closing';

export interface CommandBarState {
  state: BarState;
  isOpen: boolean;
  open: () => void;
  close: () => void;
  toggle: () => void;
  /** Called when the exit animation ends. */
  closed: () => void;
}

export function useCommandBar(): CommandBarState {
  const [state, setState] = useState<BarState>('closed');
  const open = useCallback(() => setState('open'), []);
  const close = useCallback(() => setState((s) => (s === 'open' ? 'closing' : s)), []);
  const toggle = useCallback(() => setState((s) => (s === 'open' ? 'closing' : 'open')), []);
  const closed = useCallback(() => setState((s) => (s === 'closing' ? 'closed' : s)), []);
  return { state, isOpen: state === 'open', open, close, toggle, closed };
}

const GROUPS = [
  { id: 'navigate', label: 'command-group-navigate' },
  { id: 'actions', label: 'command-group-actions' },
] as const;

/** Key prefix of a search result in the list. */
const HIT = 'hit:';
/** Search results shown in the bar. */
const MAX_HITS = 6;
/** Fewer characters than this search nothing: a single letter matches too much. */
const MIN_QUERY = 2;
/** Pause after typing before searching. */
const SEARCH_DELAY_MS = 150;

/** Sources whose text contains what is typed, read after a short pause. */
function useSourceSearch(input: string): SearchResult[] {
  const words = input.trim();
  const searchable = words.length >= MIN_QUERY && isTauri();
  // The results of the last search done; kept while the next one runs.
  const [found, setFound] = useState<SearchResult[]>([]);
  useEffect(() => {
    if (!searchable) return;
    let active = true;
    const timer = window.setTimeout(() => {
      searchSources(words, { limit: MAX_HITS })
        .then((results) => {
          if (active) setFound(results);
        })
        .catch((error: unknown) => {
          console.error('Cannot search the sources', error);
          if (active) setFound([]);
        });
    }, SEARCH_DELAY_MS);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [words, searchable]);
  return searchable ? found : [];
}

interface CommandBarProps {
  bar: CommandBarState;
  commands: Command[];
  /** Opens a source found by the search, a meeting at the moment of its first hit. */
  onOpenSource: (source: SourceRow, atMs?: number) => void;
}

/**
 * Raycast-like bar (⌘K) to move around the app and run commands; what is typed also
 * searches the sources.
 */
export function CommandBar({ bar, commands, onOpenSource }: CommandBarProps) {
  const { t } = useI18n();
  const { contains } = useFilter({ sensitivity: 'base' });
  const format = useSourceFormat();
  const [input, setInput] = useState('');
  const results = useSourceSearch(input);

  const run = (key: Key) => {
    bar.close();
    const id = String(key);
    if (id.startsWith(HIT)) {
      const result = results.find((r) => r.source.id === id.slice(HIT.length));
      if (!result) return;
      const moment = result.hits.find((hit) => hit.atMs !== null)?.atMs ?? undefined;
      onOpenSource(result.source, moment);
      return;
    }
    commands.find((c) => c.id === key)?.run();
  };

  return (
    <ModalOverlay
      className="cmd-overlay"
      isDismissable
      isOpen={bar.state === 'open'}
      // Keeps the bar mounted until its exit animation ends.
      isExiting={bar.state === 'closing'}
      onOpenChange={(isOpen) => {
        if (!isOpen) bar.close();
      }}
    >
      <motion.div
        className="cmd-scrim"
        aria-hidden
        initial={{ opacity: 0 }}
        animate={
          bar.state === 'closing'
            ? { opacity: 0, transition: { duration: duration.d1 } }
            : { opacity: 1, transition: { duration: duration.d2 } }
        }
      />
      <Modal className="cmd-modal">
        <motion.div
          className="cmd"
          initial="hidden"
          animate={bar.state === 'closing' ? 'exit' : 'visible'}
          variants={{
            hidden: { opacity: 0, y: -10, scale: 0.96 },
            visible: { opacity: 1, y: 0, scale: 1, transition: spring.pop },
            exit: {
              opacity: 0,
              y: -6,
              scale: 0.98,
              transition: { duration: duration.d1, ease: ease.in },
            },
          }}
          onAnimationComplete={(definition) => {
            if (definition === 'exit') {
              bar.closed();
              // The next opening starts empty.
              setInput('');
            }
          }}
        >
          <Dialog className="cmd-dialog" aria-label={t('command-bar-label')}>
            <Autocomplete
              inputValue={input}
              onInputChange={setInput}
              // Search results are already about what is typed; commands are filtered by name.
              filter={(text, typed, node) =>
                String(node.key).startsWith(HIT) || contains(text, typed)
              }
            >
              <TextField className="cmd-field" aria-label={t('command-input')} autoFocus>
                <MagnifyingGlassIcon className="cmd-field-ic" aria-hidden />
                <Input className="cmd-input" placeholder={t('command-input')} />
                <kbd className="kbd">{t('key-escape')}</kbd>
              </TextField>
              <Menu
                className="cmd-list"
                aria-label={t('command-bar-label')}
                onAction={run}
                renderEmptyState={() => (
                  <div className="cmd-empty">
                    <span className="cmd-empty-title">{t('command-empty-title')}</span>
                    <span>{t('command-empty-body')}</span>
                  </div>
                )}
              >
                {GROUPS.map((group) => (
                  <MenuSection key={group.id} id={group.id} className="cmd-group">
                    <Header className="cmd-group-title">{t(group.label)}</Header>
                    {commands
                      .filter((command) => command.group === group.id)
                      .map(({ id, label, icon: CommandIcon, shortcut }) => (
                        <MenuItem key={id} id={id} className="cmd-item" textValue={label}>
                          <CommandIcon className="cmd-item-ic" aria-hidden />
                          <span className="cmd-item-label">{label}</span>
                          {shortcut && <Keyboard className="kbd">{shortcut}</Keyboard>}
                        </MenuItem>
                      ))}
                  </MenuSection>
                ))}
                {results.length > 0 && (
                  <MenuSection id="sources" className="cmd-group">
                    <Header className="cmd-group-title">{t('command-group-sources')}</Header>
                    {results.map((result) => {
                      const row = result.source;
                      const SourceIcon = row.kind === 'meeting' ? WaveformIcon : NoteIcon;
                      const title = result.hits.find((hit) => hit.kind === 'title');
                      const passage = result.hits.find((hit) => hit.kind !== 'title');
                      const name = format.name(row);
                      return (
                        <MenuItem
                          key={row.id}
                          id={`${HIT}${row.id}`}
                          className="cmd-item cmd-hit"
                          textValue={name}
                        >
                          <SourceIcon className="cmd-item-ic" aria-hidden />
                          <span className="cmd-hit-text">
                            <span className="cmd-item-label">
                              {title ? <Highlighted text={title.excerpt} /> : name}
                            </span>
                            {passage && (
                              <span className="cmd-hit-excerpt">
                                <Highlighted text={passage.excerpt} />
                              </span>
                            )}
                          </span>
                          {passage?.atMs != null && row.kind === 'meeting' && (
                            <span className="cmd-hit-at">{formatClock(passage.atMs)}</span>
                          )}
                        </MenuItem>
                      );
                    })}
                  </MenuSection>
                )}
              </Menu>
            </Autocomplete>
            <footer className="cmd-foot" aria-hidden>
              <span>
                <kbd className="kbd">↑</kbd>
                <kbd className="kbd">↓</kbd>
                {t('command-hint-move')}
              </span>
              <span>
                <kbd className="kbd">↵</kbd>
                {t('command-hint-run')}
              </span>
              <span>
                <kbd className="kbd">{t('key-escape')}</kbd>
                {t('command-hint-close')}
              </span>
            </footer>
          </Dialog>
        </motion.div>
      </Modal>
    </ModalOverlay>
  );
}
