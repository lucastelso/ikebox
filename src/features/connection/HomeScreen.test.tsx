import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { Connection, CredentialsStatus, ProfileList, UiError } from '@/bindings';
import { HomeScreen } from './HomeScreen';

const profiles: ProfileList = {
  profiles: [
    {
      id: 'trabalho',
      name: 'VPN do trabalho',
      gateway: 'vpn.example.com',
      usesXauth: true,
      remoteTs: ['192.0.2.0/24'],
    },
  ],
  broken: [],
};

const connection: Connection = {
  profileId: 'trabalho',
  established: true,
  gateway: '203.0.113.10',
  vip: '198.51.100.10',
  remoteTs: ['192.0.2.0/24'],
  establishedSecs: 3,
  bytesIn: 2048,
  bytesOut: 1024,
};

interface Backend {
  credentials?: Partial<CredentialsStatus>;
  connectError?: UiError;
  list?: ProfileList;
}

/** Simula o Rust e registra as chamadas de `connect`. */
function mockBackend({ credentials, connectError, list = profiles }: Backend = {}) {
  const connectCalls: Array<Record<string, unknown>> = [];
  let connected = false;
  mockIPC((cmd, args) => {
    switch (cmd) {
      case 'list_profiles':
        return list;
      case 'status':
        return connected ? connection : null;
      case 'credentials_status':
        return {
          required: ['psk', 'xauthUser', 'xauthPassword'],
          stored: ['psk', 'xauthUser', 'xauthPassword'],
          maskedUser: 'us***lo',
          keyringAvailable: true,
          ...credentials,
        } satisfies CredentialsStatus;
      case 'connect':
        connectCalls.push(args as Record<string, unknown>);
        if (connectError) throw connectError;
        connected = true;
        return connection;
      case 'disconnect':
        connected = false;
        return true;
      default:
        return null;
    }
  });
  return { connectCalls };
}

describe('HomeScreen', () => {
  it('conecta direto quando as credenciais estão guardadas', async () => {
    const { connectCalls } = mockBackend();
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Conectar' }));

    expect(await screen.findByText('Conectado')).toBeInTheDocument();
    expect(screen.getByText('198.51.100.10')).toBeInTheDocument();
    expect(connectCalls).toEqual([{ profileId: 'trabalho', credentials: {}, remember: false }]);
    expect(screen.getByRole('button', { name: 'Desconectar' })).toBeInTheDocument();
  });

  it('pede só as credenciais que faltam e envia com "lembrar"', async () => {
    const { connectCalls } = mockBackend({ credentials: { stored: ['psk'] } });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Conectar' }));

    const dialog = await screen.findByRole('dialog');
    expect(screen.queryByLabelText('PSK do grupo')).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText('Usuário'), 'usuario.exemplo');
    await userEvent.type(screen.getByLabelText('Senha'), 'senha-falsa');
    await userEvent.click(screen.getByRole('button', { name: 'Conectar', hidden: false }));

    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(connectCalls).toEqual([
      {
        profileId: 'trabalho',
        credentials: { xauthUser: 'usuario.exemplo', xauthPassword: 'senha-falsa' },
        remember: true,
      },
    ]);
  });

  it('mostra a causa provável e oferece corrigir as credenciais', async () => {
    mockBackend({
      connectError: {
        kind: 'negotiation',
        message: 'Usuário ou senha XAuth incorretos',
        details: ['charon: establishing CHILD_SA failed'],
      },
    });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Conectar' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Usuário ou senha XAuth incorretos');
    expect(screen.getByText('Desconectado')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Informar credenciais' }));
    expect(await screen.findByLabelText('PSK do grupo')).toBeInTheDocument();
  });

  it('explica o que fazer quando não há perfis', async () => {
    mockBackend({ list: { profiles: [], broken: [] } });
    render(<HomeScreen />);
    expect(await screen.findByText('Nenhum perfil ainda')).toBeInTheDocument();
  });
});
