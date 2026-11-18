import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { DiagnosticCheck } from '@/bindings';
import { I18nProvider } from '@/i18n';
import { SystemScreen } from './SystemScreen';

const checks: DiagnosticCheck[] = [
  { name: 'Docker', level: 'ok', detail: 'reachable', hint: null },
  {
    name: 'UDP ports 500/4500',
    level: 'fail',
    detail: 'in use: [500]',
    hint: 'another IPsec VPN is using the ports',
  },
];

describe('SystemScreen', () => {
  it('shows the diagnostics with the hints for problems', async () => {
    mockIPC((cmd) => {
      if (cmd === 'run_diagnostics') return checks;
      if (cmd === 'app_info') return { version: '0.0.0', engineImage: 'engine:teste' };
      return null;
    });
    render(<SystemScreen />);
    expect(await screen.findByText('1 problem prevents connecting.')).toBeInTheDocument();
    expect(screen.getByText('→ another IPsec VPN is using the ports')).toBeInTheDocument();
    // Screen readers hear the level in the interface language, not "fail".
    expect(screen.getByLabelText('Problem')).toBeInTheDocument();
    expect(screen.queryByLabelText('fail')).toBeNull();
    expect(await screen.findByText(/engine:teste/)).toBeInTheDocument();
  });

  it('changes the theme and applies it to the document', async () => {
    mockIPC((cmd) => (cmd === 'run_diagnostics' ? [] : null));
    render(<SystemScreen />);
    const theme = within(screen.getByRole('group', { name: 'Theme' }));
    await userEvent.click(theme.getByRole('radio', { name: 'Dark' }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    await userEvent.click(theme.getByRole('radio', { name: 'System' }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it('switches the language, remembers it and tells Rust', async () => {
    const languages: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'set_language') languages.push((args as { lang: string }).lang);
      return cmd === 'run_diagnostics' ? [] : null;
    });
    render(
      <I18nProvider initial="en">
        <SystemScreen />
      </I18nProvider>,
    );
    expect(screen.getByText('Diagnostics')).toBeInTheDocument();
    const language = within(screen.getByRole('group', { name: 'Language' }));
    await userEvent.click(language.getByRole('radio', { name: 'Português' }));
    expect(await screen.findByText('Diagnóstico')).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Idioma' })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('pt-BR');
    expect(localStorage.getItem('ikebox:language')).toBe('pt');
    expect(languages).toEqual(['en', 'pt']);
    await userEvent.click(
      within(screen.getByRole('group', { name: 'Idioma' })).getByRole('radio', { name: 'Sistema' }),
    );
    expect(localStorage.getItem('ikebox:language')).toBeNull();
    expect(await screen.findByText('Diagnostics')).toBeInTheDocument();
  });

  it('closing to the tray and the update notice are settings kept by Rust', async () => {
    const calls: Array<[string, unknown]> = [];
    let settings = {
      closeToTray: true,
      updateCheck: false,
      updateCheckAvailable: true,
      availableUpdate: null as string | null,
    };
    mockIPC((cmd, args) => {
      if (cmd === 'run_diagnostics') return [];
      if (cmd === 'app_settings') return settings;
      if (cmd === 'set_close_to_tray' || cmd === 'set_update_check') {
        const enabled = (args as { enabled: boolean }).enabled;
        calls.push([cmd, enabled]);
        settings =
          cmd === 'set_close_to_tray'
            ? { ...settings, closeToTray: enabled }
            : { ...settings, updateCheck: enabled };
        return settings;
      }
      return null;
    });
    render(<SystemScreen />);
    const tray = await screen.findByLabelText('Closing the window keeps the app in the tray');
    expect(tray).toBeChecked();
    const update = screen.getByLabelText('Tell me when a new version is out');
    expect(update).not.toBeChecked();
    expect(screen.getByText(/Nothing is downloaded or installed/)).toBeInTheDocument();
    await userEvent.click(update);
    await userEvent.click(tray);
    expect(calls).toEqual([
      ['set_update_check', true],
      ['set_close_to_tray', false],
    ]);
    expect(screen.getByLabelText('Tell me when a new version is out')).toBeChecked();
  });

  it('hides the update notice option when it is not built in', async () => {
    mockIPC((cmd) => {
      if (cmd === 'run_diagnostics') return [];
      if (cmd === 'app_settings')
        return {
          closeToTray: true,
          updateCheck: false,
          updateCheckAvailable: false,
          availableUpdate: null,
        };
      return null;
    });
    render(<SystemScreen />);
    expect(await screen.findByText('Behavior')).toBeInTheDocument();
    expect(screen.queryByLabelText('Tell me when a new version is out')).not.toBeInTheDocument();
  });
});
