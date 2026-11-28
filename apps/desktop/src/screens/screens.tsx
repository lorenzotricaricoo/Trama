import { useMemo } from 'react';
import { useI18n } from '../i18n/I18nProvider';
import type { ScreenId } from '../shell/navigation';
import { SettingsScreen } from '../settings/SettingsScreen';
import { HouseIcon, StackIcon } from '../ui/icons';
import { PlaceholderScreen } from './PlaceholderScreen';

// Home and Sources arrive with #35.

function HomeScreen() {
  const { t, locale } = useI18n();
  const today = useMemo(
    () =>
      new Intl.DateTimeFormat(locale, { weekday: 'long', day: 'numeric', month: 'long' }).format(
        new Date(),
      ),
    [locale],
  );
  return (
    <PlaceholderScreen
      kicker={today}
      title={t('welcome-title')}
      description={t('home-description')}
      icon={HouseIcon}
      emptyTitle={t('home-empty-title')}
      emptyBody={t('home-empty-body')}
    />
  );
}

function SourcesScreen() {
  const { t } = useI18n();
  return (
    <PlaceholderScreen
      title={t('nav-sources')}
      description={t('sources-description')}
      icon={StackIcon}
      emptyTitle={t('sources-empty-title')}
      emptyBody={t('sources-empty-body')}
    />
  );
}

export function Screen({ id }: { id: ScreenId }) {
  switch (id) {
    case 'home':
      return <HomeScreen />;
    case 'sources':
      return <SourcesScreen />;
    case 'settings':
      return <SettingsScreen />;
  }
}
