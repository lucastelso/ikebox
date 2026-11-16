import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type {
  ComponentInfo,
  Connection,
  CredentialsStatus,
  ProfileList,
  UiError,
} from '@/bindings';
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
  component?: ComponentInfo;
}

const missingComponent: ComponentInfo = {
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

/** Simula o Rust e registra as chamadas de `connect`. */
function mockBackend({ credentials, connectError, list = profiles, component }: Backend = {}) {
  const connectCalls: Array<Record<string, unknown>> = [];
  let connected = false;
  let componentInfo = component;
  mockIPC((cmd, args) => {
    switch (cmd) {
      case 'component_status':
        return componentInfo ?? null;
      case 'install_component':
        componentInfo = { ...missingComponent, state: 'ready', installedVersion: '0.1.0' };
        return componentInfo;
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
    expect(screen.getByRole('button', { name: 'Importar perfil' })).toBeInTheDocument();
  });

  it('oferece instalar o componente de rede e some depois de instalado', async () => {
    mockBackend({ component: missingComponent });
    render(<HomeScreen />);
    const banner = await screen.findByRole('region', { name: 'Componente de rede' });
    expect(banner).toHaveTextContent('Falta um passo');
    await userEvent.click(screen.getByRole('button', { name: 'Instalar agora' }));
    await waitFor(() =>
      expect(screen.queryByRole('region', { name: 'Componente de rede' })).not.toBeInTheDocument(),
    );
  });

  it('erro de componente ausente oferece a instalação', async () => {
    mockBackend({
      component: missingComponent,
      connectError: {
        kind: 'componentMissing',
        message: 'O computador ainda não está pronto para conectar.',
        details: ['Componente de rede: não instalado'],
      },
    });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Conectar' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('não está pronto');
    await userEvent.click(screen.getByRole('button', { name: 'Instalar componente de rede' }));
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
  });
});
