import { useCallback, type ReactNode } from 'react';
import {
  Button,
  Dialog,
  Heading,
  Label,
  ListBox,
  ListBoxItem,
  Modal,
  ModalOverlay,
  Popover,
  Radio,
  RadioGroup,
  Select,
  SelectValue,
  Switch,
  Text,
  type Key,
} from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import { CaretDownIcon, WarningIcon } from '../ui/icons';
import type { StartupNotice } from './api';

/** Translates an error code: a settings message, a provider message, or a generic one. */
export function useErrorText() {
  const { t, has } = useI18n();
  return useCallback(
    (code: string) => {
      const id = code.replace(/_/g, '-');
      for (const candidate of [`settings-error-${id}`, `provider-error-${id}`]) {
        if (has(candidate)) return t(candidate);
      }
      return t('settings-error-internal');
    },
    [t, has],
  );
}

/** A problem found at start, as a sentence (`settings-notice-<code>`). */
export function useNoticeText() {
  const { t } = useI18n();
  return useCallback(
    (notice: StartupNotice) =>
      t(`settings-notice-${notice.code.replace(/_/g, '-')}`, { path: notice.path ?? '' }),
    [t],
  );
}

/** An error shown next to what failed, announced by screen readers. */
export function ErrorText({ code }: { code: string | null }) {
  const errorText = useErrorText();
  if (code === null) return null;
  return (
    <p className="set-error" role="alert">
      <WarningIcon aria-hidden />
      <span>{errorText(code)}</span>
    </p>
  );
}

/** A group of settings: a title, an optional description, then its rows. */
export function Group({
  title,
  description,
  action,
  children,
}: {
  title: string;
  description?: ReactNode;
  /** A control beside the title, such as Refresh. */
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="set-group" aria-label={title}>
      <div className="set-group-head">
        <div className="set-group-text">
          <h2 className="set-group-title">{title}</h2>
          {description && <p className="set-group-desc">{description}</p>}
        </div>
        {action}
      </div>
      <div className="set-group-body">{children}</div>
    </section>
  );
}

/** One row of a group: the label and description on the left, the control on the right. */
export function Row({
  label,
  description,
  children,
  wide,
}: {
  label?: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  /** The control goes under the text, across the whole width. */
  wide?: boolean;
}) {
  return (
    <div className="set-row" data-wide={wide || undefined}>
      {(label || description) && (
        <div className="set-row-text">
          {label && <div className="set-row-label">{label}</div>}
          {description && <div className="set-row-desc">{description}</div>}
        </div>
      )}
      <div className="set-row-control">{children}</div>
    </div>
  );
}

/** A row whose control is a switch on the right, as in the macOS settings. */
export function ToggleRow({
  label,
  description,
  isSelected,
  isDisabled,
  onChange,
}: {
  label: string;
  description?: ReactNode;
  isSelected: boolean;
  isDisabled?: boolean;
  onChange: (selected: boolean) => void;
}) {
  return (
    <Switch
      className="set-row set-toggle"
      isSelected={isSelected}
      isDisabled={isDisabled}
      onChange={onChange}
    >
      <span className="set-row-text">
        <span className="set-row-label">{label}</span>
        {description && <span className="set-row-desc">{description}</span>}
      </span>
      <span className="set-switch-track" aria-hidden>
        <span className="set-switch-thumb" />
      </span>
    </Switch>
  );
}

/** A choice among a few options, shown as segments (the macOS segmented control). */
export function Segmented<T extends string>({
  label,
  options,
  value,
  onChange,
  labelHidden,
}: {
  label: string;
  options: readonly { id: T; label: ReactNode }[];
  value: T;
  onChange: (value: T) => void;
  labelHidden?: boolean;
}) {
  return (
    <RadioGroup
      className="set-radios"
      orientation="horizontal"
      value={value}
      onChange={(next) => onChange(next as T)}
      aria-label={labelHidden ? label : undefined}
    >
      {!labelHidden && <Label className="set-row-label">{label}</Label>}
      <div className="set-segments">
        {options.map((option) => (
          <Radio key={option.id} value={option.id} className="set-segment">
            {option.label}
          </Radio>
        ))}
      </div>
    </RadioGroup>
  );
}

export interface Option {
  id: string;
  label: string;
}

/** A labelled select (React Aria), keyboard and VoiceOver friendly. */
export function SelectField({
  label,
  description,
  options,
  value,
  onChange,
  isDisabled,
  placeholder,
  mono,
  labelHidden,
}: {
  label: string;
  /** The label is shown elsewhere (by the row); it still names the control. */
  labelHidden?: boolean;
  description?: string;
  options: readonly Option[];
  value: string | null;
  onChange: (value: string) => void;
  isDisabled?: boolean;
  placeholder?: string;
  /** Values are identifiers (model names), shown in the monospaced font. */
  mono?: boolean;
}) {
  return (
    <Select
      className="set-select"
      data-mono={mono || undefined}
      selectedKey={value}
      onSelectionChange={(key: Key | null) => {
        if (key !== null) onChange(String(key));
      }}
      isDisabled={isDisabled}
      placeholder={placeholder}
      aria-label={labelHidden ? label : undefined}
    >
      {!labelHidden && <Label className="set-row-label">{label}</Label>}
      {description && (
        <Text slot="description" className="set-row-desc">
          {description}
        </Text>
      )}
      <Button className="set-select-btn">
        <SelectValue className="set-select-value" />
        <CaretDownIcon aria-hidden />
      </Button>
      <Popover className="set-popover">
        <ListBox className="set-listbox" items={options}>
          {(option) => (
            <ListBoxItem className="set-option" id={option.id} textValue={option.label}>
              {option.label}
            </ListBoxItem>
          )}
        </ListBox>
      </Popover>
    </Select>
  );
}

/** A modal that asks to confirm an action, with the destructive choice styled as such. */
export function ConfirmDialog({
  isOpen,
  title,
  body,
  confirm,
  destructive,
  onConfirm,
  onCancel,
}: {
  isOpen: boolean;
  title: string;
  body: ReactNode;
  confirm: string;
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  return (
    <ModalOverlay
      className="set-overlay"
      isOpen={isOpen}
      isDismissable
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <Modal className="set-modal">
        <Dialog className="set-dialog" role="alertdialog">
          <Heading slot="title" className="set-dialog-title">
            {title}
          </Heading>
          <div className="set-dialog-body">{body}</div>
          <div className="set-dialog-actions">
            <Button className="set-btn" onPress={onCancel} autoFocus>
              {t('settings-cancel')}
            </Button>
            <Button
              className="set-btn"
              data-variant={destructive ? 'danger' : 'primary'}
              onPress={onConfirm}
            >
              {confirm}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
