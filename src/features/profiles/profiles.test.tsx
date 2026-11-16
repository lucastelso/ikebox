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
  it('imports a swanctl.conf with the given DNS', async () => {
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

    expect(screen.getByText(/Secrets in the file are not read/)).toBeInTheDocument();
    const id = screen.getByLabelText('Identifier');
    await userEvent.clear(id);
    await userEvent.type(id, 'trabalho');
    const name = screen.getByLabelText('Name');
    await userEvent.clear(name);
    await userEvent.type(name, 'VPN do trabalho');
    await userEvent.type(screen.getByLabelText('DNS servers'), '192.0.2.53');
    await userEvent.type(screen.getByLabelText('Internal domains'), '~corp.example.com');
    await userEvent.click(screen.getByRole('button', { name: 'Import' }));

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

  it('asks to confirm the replacement when the id already exists', async () => {
    mockIPC(() => null);
    render(
      <ImportDialog
        preview={{ ...swanctlPreview, kind: 'json', exists: true }}
        onClose={() => {}}
        onImported={() => {}}
      />,
    );
    const importButton = screen.getByRole('button', { name: 'Import' });
    expect(importButton).toBeDisabled();
    await userEvent.click(screen.getByLabelText(/Replace it/));
    expect(importButton).toBeEnabled();
    expect(screen.queryByLabelText('DNS servers')).not.toBeInTheDocument();
  });

  it('shows the problem with an invalid file without allowing the import', () => {
    mockIPC(() => null);
    render(
      <ImportDialog
        preview={{ ...swanctlPreview, error: 'Profile with invalid JSON' }}
        onClose={() => {}}
        onImported={() => {}}
      />,
    );
    expect(screen.getByRole('alert')).toHaveTextContent('Profile with invalid JSON');
    expect(screen.queryByRole('button', { name: 'Import' })).not.toBeInTheDocument();
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

  it('lists, exports and deletes profiles (with their credentials)', async () => {
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

    await userEvent.click(screen.getByRole('button', { name: 'Export VPN do trabalho' }));
    expect(calls).toContainEqual({ cmd: 'export_profile', args: { profileId: 'trabalho' } });

    await userEvent.click(screen.getByRole('button', { name: 'Delete VPN do trabalho' }));
    await userEvent.click(await screen.findByRole('button', { name: 'Delete' }));
    expect(await screen.findByText('No profiles yet.')).toBeInTheDocument();
    expect(calls).toContainEqual({
      cmd: 'delete_profile',
      args: { profileId: 'trabalho', forgetCredentials: true },
    });
  });
});
