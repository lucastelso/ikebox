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
    expect(localStorage.getItem('ipsec-client:language')).toBe('pt');
    expect(languages).toEqual(['en', 'pt']);
    await userEvent.click(
      within(screen.getByRole('group', { name: 'Idioma' })).getByRole('radio', { name: 'Sistema' }),
    );
    expect(localStorage.getItem('ipsec-client:language')).toBeNull();
    expect(await screen.findByText('Diagnostics')).toBeInTheDocument();
  });
});
