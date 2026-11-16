import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { ImportPreview, ProfileList } from '@/bindings';
import { ImportDialog } from './ImportDialog';
import { ProfilesScreen } from './ProfilesScreen';

const swanctlPreview: ImportPreview = {
  token: 'import-0',
  fileName: 'vpn-legada.conf',
  kind: 'swanctl',
  suggestedId: 'vpn-trabalho',
  suggestedName: 'vpn_trabalho',
  gateway: '203.0.113.10',
  remoteTs: ['192.0.2.0/24'],
  warnings: ["'updown' foi ignorado (o app usa o próprio)"],
  error: null,
  exists: false,
};

function mockCalls() {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  return calls;
}

describe('ImportDialog', () => {
  it('importa um swanctl.conf com o DNS informado', async () => {
    const calls = mockCalls();
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'confirm_import') {
        return {
          id: 'trabalho',
          name: 'VPN do trabalho',
          gateway: '203.0.113.10',
          usesXauth: true,
          remoteTs: [],
        };
      }
      return null;
    });
    const onImported = vi.fn();
    render(<ImportDialog preview={swanctlPreview} onClose={() => {}} onImported={onImported} />);

    expect(screen.getByText(/segredos do arquivo não são lidos/)).toBeInTheDocument();
    const id = screen.getByLabelText('Identificador');
    await userEvent.clear(id);
    await userEvent.type(id, 'trabalho');
    const name = screen.getByLabelText('Nome');
    await userEvent.clear(name);
    await userEvent.type(name, 'VPN do trabalho');
    await userEvent.type(screen.getByLabelText('Servidores DNS'), '192.0.2.53');
    await userEvent.type(screen.getByLabelText('Domínios internos'), '~corp.example.com');
    await userEvent.click(screen.getByRole('button', { name: 'Importar' }));

    await waitFor(() => expect(onImported).toHaveBeenCalled());
    expect(calls).toContainEqual({
      cmd: 'confirm_import',
      args: {
        token: 'import-0',
        options: {
          id: 'trabalho',
          name: 'VPN do trabalho',
          dnsServers: ['192.0.2.53'],
          dnsDomains: ['~corp.example.com'],
          useReceivedDns: false,
          overwrite: false,
        },
      },
    });
  });

  it('exige confirmar a substituição quando o id já existe', async () => {
    mockIPC(() => null);
    render(
      <ImportDialog
        preview={{ ...swanctlPreview, kind: 'json', exists: true }}
        onClose={() => {}}
        onImported={() => {}}
      />,
    );
    const importButton = screen.getByRole('button', { name: 'Importar' });
    expect(importButton).toBeDisabled();
    await userEvent.click(screen.getByLabelText(/Substituir/));
    expect(importButton).toBeEnabled();
    expect(screen.queryByLabelText('Servidores DNS')).not.toBeInTheDocument();
  });

  it('mostra o problema de um arquivo inválido sem permitir importar', () => {
    mockIPC(() => null);
    render(
      <ImportDialog
        preview={{ ...swanctlPreview, error: 'Perfil com JSON inválido' }}
        onClose={() => {}}
        onImported={() => {}}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('Perfil com JSON inválido');
    expect(screen.queryByRole('button', { name: 'Importar' })).not.toBeInTheDocument();
  });
});

describe('ProfilesScreen', () => {
  const list: ProfileList = {
    profiles: [
      {
        id: 'trabalho',
        name: 'VPN do trabalho',
        gateway: '203.0.113.10',
        usesXauth: true,
        remoteTs: [],
      },
    ],
    broken: [{ file: 'quebrado.json', error: 'JSON inválido' }],
  };

  it('lista, exporta e exclui perfis (com as credenciais)', async () => {
    const calls = mockCalls();
    let current = list;
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === 'list_profiles') return current;
      if (cmd === 'export_profile') return 'trabalho.json';
      if (cmd === 'delete_profile') {
        current = { profiles: [], broken: [] };
        return null;
      }
      return null;
    });
    render(<ProfilesScreen onImport={() => {}} />);

    expect(await screen.findByText('VPN do trabalho')).toBeInTheDocument();
    expect(screen.getByText(/quebrado.json/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Exportar VPN do trabalho' }));
    expect(calls).toContainEqual({ cmd: 'export_profile', args: { profileId: 'trabalho' } });

    await userEvent.click(screen.getByRole('button', { name: 'Excluir VPN do trabalho' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Excluir' }));
    expect(await screen.findByText('Nenhum perfil ainda.')).toBeInTheDocument();
    expect(calls).toContainEqual({
      cmd: 'delete_profile',
      args: { profileId: 'trabalho', forgetCredentials: true },
    });
  });
});
