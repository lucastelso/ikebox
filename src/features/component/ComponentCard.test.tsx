import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { ComponentInfo, UiError } from '@/bindings';
import { ComponentCard } from './ComponentCard';
import { useComponent } from './useComponent';

const base: ComponentInfo = {
  state: 'notInstalled',
  installedVersion: null,
  installedStrongswan: null,
  bundledVersion: '0.1.0',
  bundledStrongswan: '6.1.0',
  managedByPackage: false,
  canInstall: true,
  canUninstall: false,
  problem: null,
};

const ready: ComponentInfo = {
  ...base,
  state: 'ready',
  installedVersion: '0.1.0',
  installedStrongswan: '6.1.0',
  canUninstall: true,
};

function Harness() {
  const c = useComponent();
  return (
    <ComponentCard
      info={c.info}
      busy={c.busy}
      onInstall={() => void c.install()}
      onUninstall={() => void c.uninstall()}
    />
  );
}

/** Simula o Rust: estado inicial e respostas de instalar/remover. */
function mockComponent(initial: ComponentInfo, installError?: UiError) {
  const calls: string[] = [];
  let info = initial;
  mockIPC((cmd) => {
    calls.push(cmd);
    switch (cmd) {
      case 'component_status':
        return info;
      case 'install_component':
        if (installError) throw installError;
        info = ready;
        return info;
      case 'uninstall_component':
        info = { ...base, canInstall: true };
        return info;
      default:
        return null;
    }
  });
  return calls;
}

describe('ComponentCard', () => {
  it('instala e passa a mostrar a versão instalada', async () => {
    const calls = mockComponent(base);
    render(<Harness />);
    expect(await screen.findByText('Não instalado')).toBeInTheDocument();
    expect(screen.getByText('O sistema vai pedir a senha de administrador.')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Instalar' }));
    expect(await screen.findByText('Instalado e funcionando.')).toBeInTheDocument();
    expect(screen.getAllByText('IPsec Client 0.1.0 · strongSwan 6.1.0')).toHaveLength(2);
    expect(calls).toContain('install_component');
  });

  it('remove só depois de confirmar', async () => {
    const calls = mockComponent(ready);
    render(<Harness />);
    await userEvent.click(await screen.findByRole('button', { name: 'Remover' }));
    expect(await screen.findByRole('dialog')).toHaveTextContent('Remover o componente de rede?');
    expect(calls).not.toContain('uninstall_component');
    await userEvent.click(screen.getByRole('button', { name: 'Remover' }));
    await waitFor(() => expect(calls).toContain('uninstall_component'));
    expect(await screen.findByText('Não instalado')).toBeInTheDocument();
  });

  it('cancelar a senha mantém o estado', async () => {
    mockComponent(base, { kind: 'cancelled', message: 'Autorização cancelada.', details: [] });
    render(<Harness />);
    await userEvent.click(await screen.findByRole('button', { name: 'Instalar' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Instalar' })).toBeEnabled());
    expect(screen.getByText('Não instalado')).toBeInTheDocument();
  });

  it('desatualizado oferece atualizar; instalado por pacote não oferece nada', async () => {
    const { rerender } = render(
      <ComponentCard
        info={{ ...ready, state: 'outdated', bundledVersion: '0.2.0' }}
        busy={null}
        onInstall={() => {}}
        onUninstall={() => {}}
      />,
    );
    expect(screen.getByRole('button', { name: 'Atualizar' })).toBeInTheDocument();
    rerender(
      <ComponentCard
        info={{ ...ready, managedByPackage: true, canInstall: false, canUninstall: false }}
        busy={null}
        onInstall={() => {}}
        onUninstall={() => {}}
      />,
    );
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.getByText(/pacote do sistema/)).toBeInTheDocument();
  });
});
