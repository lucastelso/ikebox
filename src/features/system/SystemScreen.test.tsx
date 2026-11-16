import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { DiagnosticCheck } from '@/bindings';
import { SystemScreen } from './SystemScreen';

const checks: DiagnosticCheck[] = [
  { name: 'Docker', level: 'ok', detail: 'acessível', hint: null },
  {
    name: 'Portas UDP 500/4500',
    level: 'fail',
    detail: 'em uso: [500]',
    hint: 'outra VPN IPsec está usando as portas',
  },
];

describe('SystemScreen', () => {
  it('mostra o diagnóstico com as dicas dos problemas', async () => {
    mockIPC((cmd) => {
      if (cmd === 'run_diagnostics') return checks;
      if (cmd === 'app_info') return { version: '0.0.0', engineImage: 'engine:teste' };
      return null;
    });
    render(<SystemScreen />);
    expect(await screen.findByText('1 problema(s) impedem a conexão.')).toBeInTheDocument();
    expect(screen.getByText('→ outra VPN IPsec está usando as portas')).toBeInTheDocument();
    expect(await screen.findByText(/engine:teste/)).toBeInTheDocument();
  });

  it('troca o tema e aplica no documento', async () => {
    mockIPC((cmd) => (cmd === 'run_diagnostics' ? [] : null));
    render(<SystemScreen />);
    await userEvent.click(screen.getByRole('radio', { name: 'Escuro' }));
    expect(document.documentElement.dataset.theme).toBe('dark');
    await userEvent.click(screen.getByRole('radio', { name: 'Sistema' }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
