import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type {
  ComponentInfo,
  Connection,
  CredentialsStatus,
  ProfileList,
  UiError,
} from '@/bindings';
import { I18nProvider } from '@/i18n';
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

/** Fakes Rust and records the `connect` calls. */
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
  it('connects right away when the credentials are stored', async () => {
    const { connectCalls } = mockBackend();
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(screen.getByText('198.51.100.10')).toBeInTheDocument();
    expect(connectCalls).toEqual([{ profileId: 'trabalho', credentials: {}, remember: false }]);
    expect(screen.getByRole('button', { name: 'Disconnect' })).toBeInTheDocument();
  });

  it('asks only for the missing credentials and sends them with "remember"', async () => {
    const { connectCalls } = mockBackend({ credentials: { stored: ['psk'] } });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    const dialog = await screen.findByRole('dialog');
    // The close button speaks the interface language (it said "Fechar" in English).
    expect(within(dialog).getByRole('button', { name: 'Close' })).toBeInTheDocument();
    expect(screen.queryByLabelText('Group PSK')).not.toBeInTheDocument();
    await userEvent.type(screen.getByLabelText('Username'), 'usuario.exemplo');
    await userEvent.type(screen.getByLabelText('Password'), 'senha-falsa');
    await userEvent.click(screen.getByRole('button', { name: 'Connect', hidden: false }));

    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(connectCalls).toEqual([
      {
        profileId: 'trabalho',
        credentials: { xauthUser: 'usuario.exemplo', xauthPassword: 'senha-falsa' },
        remember: true,
      },
    ]);
  });

  it('shows the likely cause and offers to fix the credentials', async () => {
    mockBackend({
      connectError: {
        kind: 'negotiation',
        message: 'Wrong XAuth username or password',
        details: ['charon: establishing CHILD_SA failed'],
      },
    });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Connect' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Wrong XAuth username or password');
    // The top of the screen says it failed (red), and offers the fix.
    expect(screen.getByRole('heading', { name: 'Not connected' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Enter credentials' }));
    expect(await screen.findByLabelText('Group PSK')).toBeInTheDocument();
  });

  it('explains what to do when there are no profiles', async () => {
    mockBackend({ list: { profiles: [], broken: [] } });
    render(<HomeScreen />);
    expect(await screen.findByText('No profiles yet')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Import profile' })).toBeInTheDocument();
  });

  it('offers to install the network component and goes away once installed', async () => {
    mockBackend({ component: missingComponent });
    render(<HomeScreen />);
    const banner = await screen.findByRole('region', { name: 'Network component' });
    expect(banner).toHaveTextContent('One step left');
    await userEvent.click(screen.getByRole('button', { name: 'Install now' }));
    await waitFor(() =>
      expect(screen.queryByRole('region', { name: 'Network component' })).not.toBeInTheDocument(),
    );
  });

  it('a missing component error offers the installation', async () => {
    mockBackend({
      component: missingComponent,
      connectError: {
        kind: 'componentMissing',
        message: 'This computer is not ready to connect yet.',
        details: ['Network component: not installed'],
      },
    });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Connect' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('not ready');
    await userEvent.click(screen.getByRole('button', { name: 'Install network component' }));
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
  });

  it('an old component (the helper refused the request) offers to update', async () => {
    mockBackend({
      component: { ...missingComponent, state: 'ready', installedVersion: '0.1.0' },
      connectError: {
        kind: 'componentOutdated',
        message: 'The installed network component is from an older version.',
        details: [],
      },
    });
    render(<HomeScreen />);
    await userEvent.click(await screen.findByRole('button', { name: 'Connect' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('older version');
    await userEvent.click(screen.getByRole('button', { name: 'Update network component' }));
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
  });

  it('speaks Portuguese when that is the chosen language', async () => {
    mockBackend();
    render(
      <I18nProvider initial="pt">
        <HomeScreen />
      </I18nProvider>,
    );
    await userEvent.click(await screen.findByRole('button', { name: 'Conectar' }));
    expect(await screen.findByText('Conectado')).toBeInTheDocument();
    expect(screen.getByText('IP virtual')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Desconectar' })).toBeInTheDocument();
  });
});
