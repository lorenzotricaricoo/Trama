import { motion } from 'motion/react';
import { useCallback, useState } from 'react';
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
import { MagnifyingGlassIcon, type Icon } from '../ui/icons';

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

/** Raycast-like bar (⌘K) to move around the app and run commands. */
export function CommandBar({ bar, commands }: { bar: CommandBarState; commands: Command[] }) {
  const { t } = useI18n();
  const { contains } = useFilter({ sensitivity: 'base' });

  const run = (key: Key) => {
    const command = commands.find((c) => c.id === key);
    bar.close();
    command?.run();
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
            if (definition === 'exit') bar.closed();
          }}
        >
          <Dialog className="cmd-dialog" aria-label={t('command-bar-label')}>
            <Autocomplete filter={contains}>
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
