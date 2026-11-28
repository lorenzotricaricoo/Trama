import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import { App } from './App';
import { I18nProvider } from './i18n/I18nProvider';
import type { Locale } from './i18n/catalogs';

function renderApp(locale: Locale = 'en') {
  const user = userEvent.setup();
  const result = render(
    <I18nProvider locale={locale}>
      <App />
    </I18nProvider>,
  );
  return { user, ...result };
}

const nav = () => screen.getByRole('navigation', { name: 'Main' });
const link = (name: string) => within(nav()).getByRole('link', { name });
const title = (name: string) => screen.findByRole('heading', { level: 1, name });
const commandBar = () => screen.queryByRole('dialog', { name: 'Command bar' });

beforeEach(() => {
  localStorage.clear();
});

describe('App shell', () => {
  it('shows the phase 1 sidebar with accessible names', () => {
    renderApp();
    expect(screen.getByRole('complementary', { name: 'Sidebar' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Record meeting' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Commands' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Collapse sidebar' })).toBeInTheDocument();
    expect(within(nav()).getAllByRole('link')).toEqual([
      link('Home'),
      link('Sources'),
      link('Settings'),
    ]);
    // Later phases stay hidden.
    for (const later of ['Projects', 'People', 'Doubts', 'Chat']) {
      expect(screen.queryByRole('link', { name: later })).not.toBeInTheDocument();
    }
  });

  it('starts on Home', async () => {
    renderApp();
    expect(await title('Welcome to Trama')).toBeInTheDocument();
    expect(link('Home')).toHaveAttribute('aria-current', 'page');
    expect(screen.getByRole('button', { name: 'Back' })).toHaveAttribute('aria-disabled', 'true');
  });

  it('navigates from the sidebar and back', async () => {
    const { user } = renderApp();
    await user.click(link('Sources'));
    expect(await title('Sources')).toBeInTheDocument();
    expect(link('Sources')).toHaveAttribute('aria-current', 'page');
    expect(link('Home')).not.toHaveAttribute('aria-current');

    await user.click(link('Settings'));
    expect(await title('Settings')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Back' }));
    expect(await title('Sources')).toBeInTheDocument();
    await user.keyboard('{Meta>}[[{/Meta}');
    expect(await title('Welcome to Trama')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Back' })).toHaveAttribute('aria-disabled', 'true');
  });

  it('moves through the sidebar with the keyboard', async () => {
    const { user } = renderApp();
    link('Home').focus();
    await user.keyboard('{ArrowDown}');
    expect(link('Sources')).toHaveFocus();
    await user.keyboard('{ArrowDown}');
    expect(link('Settings')).toHaveFocus();
    await user.keyboard('{ArrowDown}');
    expect(link('Home')).toHaveFocus();
    await user.keyboard('{End}');
    expect(link('Settings')).toHaveFocus();
    await user.keyboard('{Enter}');
    expect(await title('Settings')).toBeInTheDocument();
  });

  it('reaches every control with Tab', async () => {
    const { user } = renderApp();
    const order = [
      screen.getByRole('button', { name: 'Collapse sidebar' }),
      screen.getByRole('button', { name: 'Commands' }),
      screen.getByRole('button', { name: 'Record meeting' }),
      link('Home'),
      link('Sources'),
      link('Settings'),
    ];
    for (const control of order) {
      await user.tab();
      expect(control).toHaveFocus();
    }
  });

  it('opens Settings with ⌘,', async () => {
    const { user } = renderApp();
    await user.keyboard('{Meta>},{/Meta}');
    expect(await title('Settings')).toBeInTheDocument();
  });

  it('keeps the focus on Back when it leads to the first screen', async () => {
    const { user } = renderApp();
    await user.click(link('Sources'));
    await title('Sources');
    const back = screen.getByRole('button', { name: 'Back' });
    back.focus();
    await user.keyboard('{Enter}');
    expect(await title('Welcome to Trama')).toBeInTheDocument();
    expect(back).toHaveAttribute('aria-disabled', 'true');
    expect(back).toHaveFocus();
    // Pressing it again does nothing.
    await user.keyboard('{Enter}');
    expect(await title('Welcome to Trama')).toBeInTheDocument();
  });

  it('goes back with ⌘ and the [ key of layouts where [ needs ⌥', async () => {
    const { user } = renderApp();
    await user.click(link('Sources'));
    await title('Sources');
    // Italian layout: the physical [ key types è.
    fireEvent.keyDown(document.body, { key: 'è', code: 'BracketLeft', metaKey: true });
    expect(await title('Welcome to Trama')).toBeInTheDocument();
  });

  it('leaves Ctrl shortcuts to text editing on macOS', () => {
    renderApp();
    // Ctrl+K deletes to the end of the line in macOS text fields.
    const notCancelled = fireEvent.keyDown(document.body, { key: 'k', ctrlKey: true });
    expect(notCancelled).toBe(true);
    expect(commandBar()).not.toBeInTheDocument();
  });

  it('uses Ctrl instead of ⌘ on other platforms', async () => {
    Object.defineProperty(navigator, 'platform', { configurable: true, value: 'Win32' });
    renderApp();
    fireEvent.keyDown(document.body, { key: 'k', metaKey: true });
    expect(commandBar()).not.toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: 'k', ctrlKey: true });
    expect(await screen.findByRole('dialog', { name: 'Command bar' })).toBeInTheDocument();
  });

  it('ignores the other shortcuts while the command bar is open', async () => {
    const { user, container } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    await screen.findByRole('dialog', { name: 'Command bar' });
    await user.keyboard('{Meta>}\\{/Meta}');
    await user.keyboard('{Meta>}{Alt>}r{/Alt}{/Meta}');
    expect(container.querySelector('.app')).not.toHaveAttribute('data-collapsed');
    // The page behind the bar is hidden from assistive technologies while it is open.
    expect(screen.getByRole('status', { hidden: true })).toBeEmptyDOMElement();
    // ⌘, still works, and closes the bar.
    await user.keyboard('{Meta>},{/Meta}');
    expect(await title('Settings')).toBeInTheDocument();
    await waitFor(() => expect(commandBar()).not.toBeInTheDocument());
  });

  it('collapses and expands the sidebar, and remembers it', async () => {
    const { user, container, unmount } = renderApp();
    const app = container.querySelector('.app');
    await user.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    expect(app).toHaveAttribute('data-collapsed');
    // Labels stay available to VoiceOver on the icon rail.
    expect(link('Sources')).toBeInTheDocument();

    await user.keyboard('{Meta>}\\{/Meta}');
    expect(app).not.toHaveAttribute('data-collapsed');
    await user.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    unmount();

    renderApp();
    expect(screen.getByRole('button', { name: 'Expand sidebar' })).toBeInTheDocument();
  });

  it('shows the label of an icon as a tooltip when collapsed', async () => {
    const { user } = renderApp();
    await user.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    await user.tab();
    await user.tab();
    await user.tab();
    expect(link('Home')).toHaveFocus();
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Home');
  });

  it('shows the tooltip on hover too', async () => {
    const { user } = renderApp();
    await user.click(screen.getByRole('button', { name: 'Collapse sidebar' }));
    await user.hover(link('Sources'));
    expect(await screen.findByRole('tooltip')).toHaveTextContent('Sources');
    await user.unhover(link('Sources'));
    await waitFor(() => expect(screen.queryByRole('tooltip')).not.toBeInTheDocument());
  });

  it('opens the command bar with ⌘K and closes it with Escape', async () => {
    const { user } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    const dialog = await screen.findByRole('dialog', { name: 'Command bar' });
    expect(within(dialog).getByRole('textbox', { name: 'Type a command' })).toHaveFocus();
    expect(within(dialog).getByRole('menuitem', { name: /Sources/ })).toBeInTheDocument();
    expect(within(dialog).getByRole('menuitem', { name: /Record meeting/ })).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() => expect(commandBar()).not.toBeInTheDocument());
  });

  it('toggles the command bar from the sidebar and with ⌘K', async () => {
    const { user } = renderApp();
    await user.click(screen.getByRole('button', { name: 'Commands' }));
    expect(await screen.findByRole('dialog', { name: 'Command bar' })).toBeInTheDocument();
    await user.keyboard('{Meta>}k{/Meta}');
    await waitFor(() => expect(commandBar()).not.toBeInTheDocument());
    // Focus goes back where it was.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Commands' })).toHaveFocus());
  });

  it('runs a command picked with the keyboard', async () => {
    const { user } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    await screen.findByRole('dialog', { name: 'Command bar' });
    await user.keyboard('sett');
    const dialog = screen.getByRole('dialog', { name: 'Command bar' });
    expect(within(dialog).getAllByRole('menuitem')).toHaveLength(1);
    await user.keyboard('{Enter}');
    expect(await title('Settings')).toBeInTheDocument();
    await waitFor(() => expect(commandBar()).not.toBeInTheDocument());
  });

  it('moves through the commands with the arrow keys', async () => {
    const { user } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    await screen.findByRole('dialog', { name: 'Command bar' });
    // ↓ focuses Home, the first command, then Sources.
    await user.keyboard('{ArrowDown}{ArrowDown}{Enter}');
    expect(await title('Sources')).toBeInTheDocument();
  });

  it('runs actions from the command bar', async () => {
    const { user, container } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    await user.click(await screen.findByRole('menuitem', { name: /Collapse sidebar/ }));
    expect(container.querySelector('.app')).toHaveAttribute('data-collapsed');
  });

  it('says when no command matches', async () => {
    const { user } = renderApp();
    await user.keyboard('{Meta>}k{/Meta}');
    await screen.findByRole('dialog', { name: 'Command bar' });
    await user.keyboard('zzz');
    expect(screen.getByText('No results')).toBeInTheDocument();
  });

  it('speaks Italian', async () => {
    renderApp('it');
    expect(await title('Benvenuto in Trama')).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Principale' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Fonti' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Chiudi la barra laterale' })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('it');
  });

  it('applies the theme tokens to the document, opaque outside the macOS app', () => {
    renderApp();
    expect(document.documentElement).toHaveClass('t-app', 't-light', 't-opaque');
  });
});
