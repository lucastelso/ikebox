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

/** Fakes Rust: initial state and install/remove replies. */
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
  it('installs and then shows the installed version', async () => {
    const calls = mockComponent(base);
    render(<Harness />);
    expect(await screen.findByText('Not installed')).toBeInTheDocument();
    expect(
      screen.getByText('The system will ask for the administrator password.'),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Install' }));
    expect(await screen.findByText('Installed and working.')).toBeInTheDocument();
    expect(screen.getAllByText('IPsec Client 0.1.0 · strongSwan 6.1.0')).toHaveLength(2);
    expect(calls).toContain('install_component');
  });

  it('removes only after confirmation', async () => {
    const calls = mockComponent(ready);
    render(<Harness />);
    await userEvent.click(await screen.findByRole('button', { name: 'Remove' }));
    expect(await screen.findByRole('dialog')).toHaveTextContent('Remove the network component?');
    expect(calls).not.toContain('uninstall_component');
    await userEvent.click(screen.getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(calls).toContain('uninstall_component'));
    expect(await screen.findByText('Not installed')).toBeInTheDocument();
  });

  it('cancelling the password keeps the state', async () => {
    mockComponent(base, { kind: 'cancelled', message: 'Authorization cancelled.', details: [] });
    render(<Harness />);
    await userEvent.click(await screen.findByRole('button', { name: 'Install' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Install' })).toBeEnabled());
    expect(screen.getByText('Not installed')).toBeInTheDocument();
  });

  it('outdated offers to update; installed by a package offers nothing', async () => {
    const { rerender } = render(
      <ComponentCard
        info={{ ...ready, state: 'outdated', bundledVersion: '0.2.0' }}
        busy={null}
        onInstall={() => {}}
        onUninstall={() => {}}
      />,
    );
    expect(screen.getByRole('button', { name: 'Update' })).toBeInTheDocument();
    rerender(
      <ComponentCard
        info={{ ...ready, managedByPackage: true, canInstall: false, canUninstall: false }}
        busy={null}
        onInstall={() => {}}
        onUninstall={() => {}}
      />,
    );
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.getByText(/system package/)).toBeInTheDocument();
  });
});
