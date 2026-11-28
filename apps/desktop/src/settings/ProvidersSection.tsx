import { useCallback, useEffect, useState } from 'react';
import {
  Button,
  Checkbox,
  Dialog,
  Form,
  Heading,
  Input,
  Label,
  Meter,
  Modal,
  ModalOverlay,
  NumberField,
  Text,
  TextField,
} from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import {
  CaretDownIcon,
  CheckIcon,
  CloudIcon,
  DesktopIcon,
  LockSimpleIcon,
  PencilSimpleIcon,
  PlusIcon,
  TrashIcon,
  WarningIcon,
} from '../ui/icons';
import {
  api,
  errorCode,
  fromMicros,
  TASKS,
  toMicros,
  type Budget,
  type LibrarySettings,
  type Provider,
  type ProviderModel,
  type TaskModel,
} from './api';
import type { UpdateLibrary } from './SettingsScreen';
import { ConfirmDialog, ErrorText, Group, Row, SelectField, ToggleRow } from './ui';

/** Currencies offered for prices and the budget; amounts are never converted. */
export const CURRENCIES = ['USD', 'EUR', 'GBP', 'CHF', 'JPY', 'CAD', 'AUD'] as const;

/** Separates provider and model in the key of a task's model choice. */
const KEY_SEPARATOR = '\n';
const NO_MODEL = '';

/** Whether a provider runs on this Mac (Ollama, LM Studio), for its icon. */
function isLocal(baseUrl: string): boolean {
  try {
    const host = new URL(baseUrl).hostname;
    return host === 'localhost' || host === '127.0.0.1' || host === '[::1]';
  } catch {
    return false;
  }
}

function useFormat(currency: string) {
  const { locale } = useI18n();
  return {
    money: (micros: number) =>
      new Intl.NumberFormat(locale, { style: 'currency', currency }).format(micros / 1e6),
    count: (value: number) =>
      new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(
        value,
      ),
    date: (ms: number) =>
      new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(ms),
  };
}

interface Props {
  library: LibrarySettings;
  onLibrary: UpdateLibrary;
}

export function ProvidersSection({ library, onLibrary }: Props) {
  const { t } = useI18n();
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [tasks, setTasks] = useState<TaskModel[]>([]);
  const [budget, setBudget] = useState<Budget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Provider | 'new' | null>(null);
  const [taskError, setTaskError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const [list, choices, status] = await Promise.all([
        api.listProviders(),
        api.taskModels(),
        api.budgetStatus(),
      ]);
      setProviders(list);
      setTasks(choices);
      setBudget(status);
      setError(null);
    } catch (err) {
      setError(errorCode(err));
    }
  }, []);

  useEffect(() => {
    // Loading from the core, which answers asynchronously.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void reload();
  }, [reload]);

  const replace = (provider: Provider) =>
    setProviders((list) => list?.map((p) => (p.id === provider.id ? provider : p)) ?? null);

  const modelOptions = [
    { id: NO_MODEL, label: t('settings-task-no-model') },
    ...(providers ?? []).flatMap((provider) =>
      provider.models.map((model) => ({
        id: `${provider.id}${KEY_SEPARATOR}${model.modelId}`,
        label: t('settings-task-model-option', {
          provider: provider.name,
          model: model.modelId,
        }),
      })),
    ),
  ];

  const chooseTask = async (task: TaskModel['task'], key: string) => {
    setTaskError(null);
    const [providerId, modelId] = key.split(KEY_SEPARATOR);
    const choice =
      key === NO_MODEL || providerId === undefined || modelId === undefined
        ? null
        : { providerId, modelId };
    try {
      await api.setTaskModel(task, choice);
      setTasks((list) => [
        ...list.filter((existing) => existing.task !== task),
        ...(choice ? [{ task, ...choice }] : []),
      ]);
    } catch (err) {
      setTaskError(errorCode(err));
    }
  };

  return (
    <>
      <Group title={t('settings-group-providers')} description={t('settings-providers-desc')}>
        <ErrorText code={error} />
        {providers?.length === 0 && <p className="set-empty">{t('settings-providers-empty')}</p>}
        {providers?.map((provider) => (
          <ProviderCard
            key={provider.id}
            provider={provider}
            currency={library.currency}
            onChange={replace}
            onEdit={() => setEditing(provider)}
            onRemoved={() => void reload()}
          />
        ))}
        <div className="set-actions">
          <Button className="set-btn" onPress={() => setEditing('new')}>
            <PlusIcon aria-hidden />
            {t('settings-provider-add')}
          </Button>
        </div>
      </Group>

      <Group title={t('settings-group-tasks')} description={t('settings-tasks-desc')}>
        {TASKS.map((task) => {
          const choice = tasks.find((existing) => existing.task === task);
          return (
            <Row
              key={task}
              label={t(`settings-task-${task}`)}
              description={t(`settings-task-${task}-desc`)}
            >
              <SelectField
                label={t(`settings-task-${task}`)}
                labelHidden
                mono
                options={modelOptions}
                value={choice ? `${choice.providerId}${KEY_SEPARATOR}${choice.modelId}` : NO_MODEL}
                onChange={(key) => void chooseTask(task, key)}
              />
            </Row>
          );
        })}
        <ErrorText code={taskError} />
      </Group>

      <BudgetGroup
        library={library}
        budget={budget}
        unpriced={unpricedTaskModels(providers ?? [], tasks)}
        onLibrary={async (change) => {
          const code = await onLibrary(change);
          if (code === null) setBudget(await api.budgetStatus().catch(() => budget));
          return code;
        }}
      />

      <ProviderDialog
        key={editing === null ? 'closed' : editing === 'new' ? 'new' : editing.id}
        provider={editing}
        onClose={() => setEditing(null)}
        onSaved={(provider) => {
          setEditing(null);
          setProviders((list) =>
            list?.some((p) => p.id === provider.id)
              ? list.map((p) => (p.id === provider.id ? provider : p))
              : [...(list ?? []), provider],
          );
        }}
      />
    </>
  );
}

function ProviderCard({
  provider,
  currency,
  onChange,
  onEdit,
  onRemoved,
}: {
  provider: Provider;
  currency: string;
  onChange: (provider: Provider) => void;
  onEdit: () => void;
  onRemoved: () => void;
}) {
  const { t } = useI18n();
  const format = useFormat(currency);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; code?: string } | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const [model, setModel] = useState<ProviderModel | 'new' | null>(null);

  const test = async () => {
    setTesting(true);
    setResult(null);
    try {
      const updated = await api.testProvider(provider.id);
      onChange(updated);
      setResult({ ok: true });
    } catch (err) {
      setResult({ ok: false, code: errorCode(err) });
    } finally {
      setTesting(false);
    }
  };

  const remove = async () => {
    setConfirmRemove(false);
    try {
      await api.removeProvider(provider.id);
    } catch (err) {
      setRemoveError(errorCode(err));
    }
    // Reloaded either way: a failed removal may still have deleted the key.
    onRemoved();
  };

  const status: { tone?: 'ok' | 'warn'; text: string } =
    provider.needsKey && !provider.hasKey
      ? { tone: 'warn', text: t('settings-key-missing') }
      : provider.catalogFetchedAt !== null
        ? { tone: 'ok', text: t('settings-provider-connected', { count: provider.models.length }) }
        : { text: t('settings-provider-untested') };

  const keyStatus = !provider.needsKey
    ? t('settings-key-none')
    : provider.hasKey
      ? t('settings-key-stored')
      : t('settings-key-missing');

  return (
    <article className="set-provider" aria-label={provider.name}>
      <div className="set-provider-head">
        <span className="set-provider-logo" aria-hidden>
          {isLocal(provider.baseUrl) ? <DesktopIcon /> : <CloudIcon />}
        </span>
        <div className="set-provider-text">
          <h3 className="set-provider-name">{provider.name}</h3>
          <div className="set-mono set-provider-url">{provider.baseUrl}</div>
        </div>
        <div className="set-status" data-tone={status.tone}>
          <span className="set-dot" aria-hidden />
          {status.text}
        </div>
        <Button
          className="set-btn"
          aria-label={t('settings-provider-edit', { name: provider.name })}
          onPress={onEdit}
        >
          {t('settings-edit')}
        </Button>
        <Button
          className="icon-btn"
          aria-label={t('settings-provider-remove', { name: provider.name })}
          onPress={() => setConfirmRemove(true)}
        >
          <TrashIcon aria-hidden />
        </Button>
      </div>
      <div className="set-provider-foot">
        <span className="set-provider-key">
          <LockSimpleIcon aria-hidden />
          {keyStatus}
        </span>
        {provider.catalogFetchedAt !== null && (
          <span className="set-provider-fetched">
            {t('settings-catalog-fetched', {
              count: provider.models.length,
              date: format.date(provider.catalogFetchedAt),
            })}
          </span>
        )}
        <Button className="set-link" onPress={() => void test()} isPending={testing}>
          {testing ? t('settings-provider-testing') : t('settings-provider-test')}
        </Button>
      </div>
      <div role="status" className="set-provider-result">
        {result?.ok && (
          <p className="set-ok">
            <CheckIcon aria-hidden />
            {t('settings-provider-test-ok', { count: provider.models.length })}
          </p>
        )}
      </div>
      {result && !result.ok && <ErrorText code={result.code ?? 'internal'} />}
      <ErrorText code={removeError} />

      <Button
        className="set-disclosure"
        aria-expanded={expanded}
        onPress={() => setExpanded((open) => !open)}
      >
        <CaretDownIcon aria-hidden className="set-disclosure-ic" />
        {t('settings-models', { count: provider.models.length })}
      </Button>
      {expanded && (
        <div className="set-models">
          {provider.models.length === 0 && (
            <p className="set-empty">{t('settings-models-empty')}</p>
          )}
          {provider.models.length > 0 && (
            <ul className="set-model-list" aria-label={t('settings-models-list')}>
              {provider.models.map((entry) => (
                <ModelRow
                  key={entry.modelId}
                  model={entry}
                  format={format}
                  onEdit={() => setModel(entry)}
                />
              ))}
            </ul>
          )}
          <div className="set-actions">
            <Button className="set-btn" onPress={() => setModel('new')}>
              <PlusIcon aria-hidden />
              {t('settings-model-add')}
            </Button>
          </div>
        </div>
      )}

      <ModelDialog
        key={model === null ? 'closed' : model === 'new' ? 'new' : model.modelId}
        providerId={provider.id}
        model={model}
        currency={currency}
        onClose={() => setModel(null)}
        onSaved={(saved) => {
          setModel(null);
          const others = provider.models.filter((m) => m.modelId !== saved.modelId);
          onChange({ ...provider, models: [...others, saved] });
        }}
      />
      <ConfirmDialog
        isOpen={confirmRemove}
        title={t('settings-provider-remove-title', { name: provider.name })}
        body={t('settings-provider-remove-body')}
        confirm={t('settings-provider-remove-confirm')}
        destructive
        onConfirm={() => void remove()}
        onCancel={() => setConfirmRemove(false)}
      />
    </article>
  );
}

function ModelRow({
  model,
  format,
  onEdit,
}: {
  model: ProviderModel;
  format: ReturnType<typeof useFormat>;
  onEdit: () => void;
}) {
  const { t } = useI18n();
  const context = model.contextTokensOverride ?? model.catalogContextTokens;
  const output = model.maxOutputTokensOverride ?? model.catalogMaxOutputTokens;
  const priced = model.inputPriceMicros !== null && model.outputPriceMicros !== null;
  return (
    <li className="set-model">
      <div className="set-model-text">
        <span className="set-model-id">{model.modelId}</span>
        <span className="set-model-meta">
          {context === null ? (
            <span className="set-chip" data-tone="warn">
              <WarningIcon aria-hidden />
              {t('settings-model-context-unknown')}
            </span>
          ) : (
            t('settings-model-limits', {
              context: format.count(context),
              output: output === null ? '–' : format.count(output),
            })
          )}
          {!model.listed && <span className="set-chip">{t('settings-model-unlisted')}</span>}
          <span>
            {priced
              ? t('settings-model-prices', {
                  input: format.money(model.inputPriceMicros ?? 0),
                  output: format.money(model.outputPriceMicros ?? 0),
                })
              : t('settings-model-no-price')}
          </span>
        </span>
      </div>
      <Button
        className="icon-btn"
        aria-label={t('settings-model-edit', { model: model.modelId })}
        onPress={onEdit}
      >
        <PencilSimpleIcon aria-hidden />
      </Button>
    </li>
  );
}

/** A number field whose empty value means "not set". */
function OptionalNumber({
  label,
  labelHidden,
  description,
  value,
  onChange,
  formatOptions,
  minValue = 0,
  autoFocus,
}: {
  label: string;
  labelHidden?: boolean;
  description?: string;
  value: number | null;
  onChange: (value: number | null) => void;
  formatOptions?: Intl.NumberFormatOptions;
  minValue?: number;
  autoFocus?: boolean;
}) {
  return (
    <NumberField
      className="set-field"
      value={value ?? NaN}
      onChange={(next) => onChange(Number.isNaN(next) ? null : next)}
      minValue={minValue}
      formatOptions={formatOptions}
      autoFocus={autoFocus}
      aria-label={labelHidden ? label : undefined}
    >
      {!labelHidden && <Label className="set-field-label">{label}</Label>}
      <Input className="set-input" />
      {description && (
        <Text slot="description" className="set-field-desc">
          {description}
        </Text>
      )}
    </NumberField>
  );
}

function ModelDialog({
  providerId,
  model,
  currency,
  onClose,
  onSaved,
}: {
  providerId: string;
  model: ProviderModel | 'new' | null;
  currency: string;
  onClose: () => void;
  onSaved: (model: ProviderModel) => void;
}) {
  const { t } = useI18n();
  const existing = model !== null && model !== 'new' ? model : null;
  const [modelId, setModelId] = useState(existing?.modelId ?? '');
  const [context, setContext] = useState(existing?.contextTokensOverride ?? null);
  const [output, setOutput] = useState(existing?.maxOutputTokensOverride ?? null);
  const [input, setInput] = useState(fromMicros(existing?.inputPriceMicros ?? null));
  const [reply, setReply] = useState(fromMicros(existing?.outputPriceMicros ?? null));
  const [error, setError] = useState<string | null>(null);
  const money = { style: 'currency', currency } as const;
  const tokens = { maximumFractionDigits: 0 } as const;

  const save = async () => {
    setError(null);
    try {
      const saved = await api.setModelSettings({
        providerId,
        modelId: modelId.trim(),
        contextTokens: context,
        maxOutputTokens: output,
        inputPriceMicros: toMicros(input),
        outputPriceMicros: toMicros(reply),
      });
      onSaved(saved);
    } catch (err) {
      setError(errorCode(err));
    }
  };

  return (
    <ModalOverlay
      className="set-overlay"
      isOpen={model !== null}
      isDismissable
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Modal className="set-modal">
        <Dialog className="set-dialog">
          <Heading slot="title" className="set-dialog-title">
            {existing
              ? t('settings-model-dialog-edit', { model: existing.modelId })
              : t('settings-model-dialog-add')}
          </Heading>
          <Form
            className="set-form"
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            {!existing && (
              <TextField
                className="set-field"
                value={modelId}
                onChange={setModelId}
                isRequired
                autoFocus
              >
                <Label className="set-field-label">{t('settings-model-id')}</Label>
                <Input className="set-input set-mono" />
              </TextField>
            )}
            <div className="set-form-grid">
              <OptionalNumber
                label={t('settings-model-context')}
                description={
                  existing?.catalogContextTokens
                    ? t('settings-model-from-catalog', { value: existing.catalogContextTokens })
                    : undefined
                }
                value={context}
                onChange={setContext}
                formatOptions={tokens}
                minValue={1}
                autoFocus={Boolean(existing)}
              />
              <OptionalNumber
                label={t('settings-model-output')}
                description={
                  existing?.catalogMaxOutputTokens
                    ? t('settings-model-from-catalog', { value: existing.catalogMaxOutputTokens })
                    : undefined
                }
                value={output}
                onChange={setOutput}
                formatOptions={tokens}
                minValue={1}
              />
              <OptionalNumber
                label={t('settings-model-input-price')}
                value={input}
                onChange={setInput}
                formatOptions={money}
              />
              <OptionalNumber
                label={t('settings-model-output-price')}
                value={reply}
                onChange={setReply}
                formatOptions={money}
              />
            </div>
            <p className="set-field-desc">{t('settings-model-dialog-hint')}</p>
            <ErrorText code={error} />
            <div className="set-dialog-actions">
              <Button className="set-btn" onPress={onClose}>
                {t('settings-cancel')}
              </Button>
              <Button className="set-btn" data-variant="primary" type="submit">
                {t('settings-save')}
              </Button>
            </div>
          </Form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function ProviderDialog({
  provider,
  onClose,
  onSaved,
}: {
  provider: Provider | 'new' | null;
  onClose: () => void;
  onSaved: (provider: Provider) => void;
}) {
  const { t } = useI18n();
  const existing = provider !== null && provider !== 'new' ? provider : null;
  const [name, setName] = useState(existing?.name ?? '');
  const [baseUrl, setBaseUrl] = useState(existing?.baseUrl ?? '');
  const [needsKey, setNeedsKey] = useState(existing?.needsKey ?? true);
  const [apiKey, setApiKey] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    setError(null);
    const input = { name, baseUrl, needsKey, apiKey: apiKey.trim() || undefined };
    try {
      const saved = existing
        ? await api.updateProvider(existing.id, input)
        : await api.addProvider(input);
      setApiKey('');
      onSaved(saved);
    } catch (err) {
      setError(errorCode(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <ModalOverlay
      className="set-overlay"
      isOpen={provider !== null}
      isDismissable
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <Modal className="set-modal">
        <Dialog className="set-dialog">
          <Heading slot="title" className="set-dialog-title">
            {existing
              ? t('settings-provider-dialog-edit', { name: existing.name })
              : t('settings-provider-dialog-add')}
          </Heading>
          <Form
            className="set-form"
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            <TextField className="set-field" value={name} onChange={setName} isRequired autoFocus>
              <Label className="set-field-label">{t('settings-provider-name')}</Label>
              <Input className="set-input" />
            </TextField>
            <TextField
              className="set-field"
              value={baseUrl}
              onChange={setBaseUrl}
              isRequired
              type="url"
            >
              <Label className="set-field-label">{t('settings-provider-url')}</Label>
              <Input className="set-input set-mono" spellCheck={false} />
              <Text slot="description" className="set-field-desc">
                {t('settings-provider-url-desc')}
              </Text>
            </TextField>
            {!existing && (
              <Checkbox className="set-check" isSelected={needsKey} onChange={setNeedsKey}>
                <span className="set-check-box" aria-hidden>
                  {needsKey && <CheckIcon weight="bold" />}
                </span>
                {t('settings-provider-needs-key')}
              </Checkbox>
            )}
            {needsKey && (
              <TextField
                className="set-field"
                value={apiKey}
                onChange={setApiKey}
                type="password"
                autoComplete="off"
              >
                <Label className="set-field-label">{t('settings-provider-key')}</Label>
                <Input className="set-input set-mono" spellCheck={false} />
                <Text slot="description" className="set-field-desc">
                  {existing?.hasKey
                    ? t('settings-provider-key-replace')
                    : t('settings-provider-key-desc')}
                </Text>
              </TextField>
            )}
            <ErrorText code={error} />
            <div className="set-dialog-actions">
              <Button className="set-btn" onPress={onClose}>
                {t('settings-cancel')}
              </Button>
              <Button className="set-btn" data-variant="primary" type="submit" isPending={saving}>
                {existing ? t('settings-save') : t('settings-provider-add-confirm')}
              </Button>
            </div>
          </Form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

/** Models chosen for a task that lack a price: an enforced budget refuses them. */
function unpricedTaskModels(providers: Provider[], tasks: TaskModel[]): string[] {
  return tasks.flatMap(({ providerId, modelId }) => {
    const model = providers
      .find((provider) => provider.id === providerId)
      ?.models.find((m) => m.modelId === modelId);
    const priced = model && model.inputPriceMicros !== null && model.outputPriceMicros !== null;
    return priced ? [] : [modelId];
  });
}

function BudgetGroup({
  library,
  budget,
  unpriced,
  onLibrary,
}: {
  library: LibrarySettings;
  budget: Budget | null;
  unpriced: string[];
  onLibrary: UpdateLibrary;
}) {
  const { t, locale } = useI18n();
  const format = useFormat(library.currency);
  const [error, setError] = useState<string | null>(null);
  const update = (change: Partial<LibrarySettings>) => {
    setError(null);
    void onLibrary(change).then(setError);
  };
  const currencyName = (code: string) =>
    new Intl.DisplayNames([locale], { type: 'currency' }).of(code) ?? code;

  const spent = budget?.spentMicros ?? 0;
  const limit = budget?.budgetMicros ?? null;

  return (
    <Group title={t('settings-group-budget')} description={t('settings-budget-desc')}>
      {budget && (
        <div className="set-budget" data-state={budget.state}>
          {limit !== null ? (
            <Meter
              className="set-meter"
              value={Math.min(spent, limit)}
              maxValue={Math.max(limit, 1)}
              valueLabel={t('settings-budget-spent-of', {
                spent: format.money(spent),
                budget: format.money(limit),
              })}
            >
              {({ percentage, valueText }) => (
                <>
                  <div className="set-meter-head">
                    <Label className="set-row-label">{t('settings-budget-month')}</Label>
                    <span className="set-meter-value">{valueText}</span>
                  </div>
                  <div className="set-meter-track">
                    <div
                      className="set-meter-fill"
                      style={{ transform: `scaleX(${percentage / 100})` }}
                    />
                  </div>
                </>
              )}
            </Meter>
          ) : (
            <div className="set-meter-head">
              <span className="set-row-label">{t('settings-budget-month')}</span>
              <span className="set-meter-value">{format.money(spent)}</span>
            </div>
          )}
          <p className="set-row-desc">
            {t('settings-budget-usage', {
              requests: budget.requests,
              input: format.count(budget.inputTokens),
              output: format.count(budget.outputTokens),
            })}
          </p>
          {budget.unpricedRequests > 0 && (
            <p className="set-row-desc">
              {t('settings-budget-unpriced', { count: budget.unpricedRequests })}
            </p>
          )}
          {(budget.state === 'warning' || budget.state === 'exceeded') && (
            <p className="set-warn" role="alert">
              <WarningIcon aria-hidden />
              {budget.state === 'warning'
                ? t('settings-budget-warning', { percent: budget.warnPercent })
                : budget.enforce
                  ? t('settings-budget-exceeded-blocked')
                  : t('settings-budget-exceeded')}
            </p>
          )}
        </div>
      )}
      <Row label={t('settings-currency')} description={t('settings-currency-desc')}>
        <SelectField
          label={t('settings-currency')}
          labelHidden
          options={CURRENCIES.map((code) => ({ id: code, label: currencyName(code) }))}
          value={library.currency}
          onChange={(currency) => update({ currency })}
        />
      </Row>
      <Row label={t('settings-budget-amount')} description={t('settings-budget-amount-desc')}>
        <OptionalNumber
          label={t('settings-budget-amount')}
          labelHidden
          value={fromMicros(library.monthlyBudgetMicros)}
          onChange={(value) => update({ monthlyBudgetMicros: toMicros(value) })}
          formatOptions={{ style: 'currency', currency: library.currency }}
        />
      </Row>
      <Row label={t('settings-budget-warn')}>
        <NumberField
          className="set-field"
          aria-label={t('settings-budget-warn')}
          value={library.warnPercent / 100}
          onChange={(value) => {
            if (!Number.isNaN(value)) update({ warnPercent: Math.round(value * 100) });
          }}
          minValue={0.01}
          maxValue={1}
          step={0.05}
          formatOptions={{ style: 'percent' }}
          isDisabled={library.monthlyBudgetMicros === null}
        >
          <Input className="set-input" />
        </NumberField>
      </Row>
      <ToggleRow
        label={t('settings-budget-enforce')}
        description={t('settings-budget-enforce-desc')}
        isSelected={library.enforceBudget}
        onChange={(enforceBudget) => update({ enforceBudget })}
        isDisabled={library.monthlyBudgetMicros === null}
      />
      {library.enforceBudget && library.monthlyBudgetMicros !== null && unpriced.length > 0 && (
        <p className="set-warn" role="alert">
          <WarningIcon aria-hidden />
          {t('settings-budget-price-missing', {
            count: unpriced.length,
            models: unpriced.join(', '),
          })}
        </p>
      )}
      <ErrorText code={error} />
    </Group>
  );
}
