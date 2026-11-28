import { Button } from 'react-aria-components';
import { useI18n } from '../i18n/I18nProvider';
import type { Job } from '../jobs/useJobs';
import { Hint } from '../ui/Hint';
import { CaretLeftIcon } from '../ui/icons';
import { JobIndicator } from './JobIndicator';
import { SHORTCUTS } from './shortcuts';

interface TopBarProps {
  title: string;
  canGoBack: boolean;
  onBack: () => void;
  jobs: readonly Job[];
}

/** Bar at the top of the sheet: Back, where you are, background work. It also moves the window. */
export function TopBar({ title, canGoBack, onBack, jobs }: TopBarProps) {
  const { t } = useI18n();
  return (
    <div className="topbar" data-tauri-drag-region>
      <Hint label={t('back')} shortcut={SHORTCUTS.back.label} placement="bottom">
        <Button
          className="icon-btn"
          aria-label={t('back')}
          isDisabled={!canGoBack}
          onPress={onBack}
        >
          <CaretLeftIcon aria-hidden />
        </Button>
      </Hint>
      <span className="topbar-title" data-tauri-drag-region>
        {title}
      </span>
      <div className="topbar-end" data-tauri-drag-region>
        <JobIndicator jobs={jobs} />
      </div>
    </div>
  );
}
