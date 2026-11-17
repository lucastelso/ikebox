import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type {
  DraftCheck,
  ProfileDraft,
  ProfileList,
  ProfileSummary,
  ProposalPresetInfo,
} from '@/bindings';
import { ProfilesScreen } from './ProfilesScreen';

const fortigate: ProfileDraft = {
  originalId: null,
  template: 'fortigate',
  id: '',
  name: '',
  gateway: '',
  ikeVersion: 1,
  aggressive: true,
  encap: true,
  ikeProposals: ['aes256-sha256-modp2048', 'aes128-sha256-modp2048'],
  espProposals: ['aes256-sha256-modp2048', 'aes128-sha256-modp2048'],
  xauth: true,
  otp: false,
  eap: false,
  ca: [],
  localId: '',
  remoteId: '',
  remoteTs: [],
  requestVip: true,
  dnsServers: [],
  dnsDomains: [],
  useReceivedDns: true,
  dnsDefaultRoute: false,
  mtu: 1400,
  dpdDelaySecs: 30,
};

const presets: Record<number, ProposalPresetInfo[]> = {
  1: [
    {
      preset: 'compatible',
      ike: fortigate.ikeProposals,
      esp: fortigate.espProposals,
      security: 'good',
    },
    {
      preset: 'legacy',
      ike: ['aes256-sha1-modp1024'],
      esp: ['aes256-sha1-modp1024'],
      security: 'weak',
    },
  ],
  2: [
    {
      preset: 'modern',
      ike: ['aes256gcm16-prfsha256-curve25519'],
      esp: ['aes256gcm16-curve25519', 'aes256gcm16'],
      security: 'good',
    },
    {
      preset: 'compatible',
      ike: ['aes256-sha256-modp2048'],
      esp: ['aes256-sha256-modp2048', 'aes256-sha256'],
      security: 'good',
    },
  ],
};

const saved: ProfileSummary = {
  id: 'escritorio',
  name: 'Escritório',
  gateway: 'vpn.example.com',
  usesXauth: true,
  otp: false,
  remoteTs: ['192.0.2.0/24'],
  security: 'weak',
};

/** Fake Rust: the check flags empty name/gateway/networks, like the real one. */
function mockEditor(list: ProfileList = { profiles: [], broken: [] }) {
  const calls: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    switch (cmd) {
      case 'list_profiles':
        return list;
      case 'new_profile_draft':
        return (args as { template: string }).template === 'ikev2Eap'
          ? {
              ...fortigate,
              template: 'ikev2Eap',
              ikeVersion: 2,
              aggressive: false,
              xauth: false,
              eap: true,
            }
          : fortigate;
      case 'ca_entries':
        return ['-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n'];
      case 'edit_profile_draft':
        return { ...fortigate, originalId: 'escritorio', id: 'escritorio', name: 'Escritório' };
      case 'proposal_presets':
        return presets[(args as { ikeVersion: number }).ikeVersion];
      case 'check_profile_draft': {
        const d = (args as { draft: ProfileDraft }).draft;
        const issues = [];
        if (!d.name.trim()) issues.push({ field: 'name', message: 'enter a name' });
        if (!d.gateway.trim()) issues.push({ field: 'gateway', message: 'enter a gateway' });
        if (d.remoteTs.every((s) => !s.trim()))
          issues.push({ field: 'remoteTs', message: 'enter a network' });
        return {
          issues,
          security: d.aggressive ? 'weak' : 'good',
          notes: d.aggressive ? [{ level: 'weak', message: 'Aggressive mode with a PSK…' }] : [],
          preset: null,
          schema: d.otp || d.eap ? 2 : 1,
          cas: d.ca.map(() => ({
            subject: 'CN=IPsec Client Test CA (RSA),O=Example',
            notAfter: '2076-11-04',
            key: 'RSA',
          })),
        } satisfies DraftCheck;
      }
      case 'save_profile_draft':
        return saved;
      default:
        return null;
    }
  });
  return calls;
}

const lastDraft = (calls: ReturnType<typeof mockEditor>, cmd: string) =>
  calls.filter((c) => c.cmd === cmd).at(-1)?.args.draft as ProfileDraft | undefined;

describe('ProfileEditor', () => {
  it('creates a profile from the FortiGate template, showing problems only on save', async () => {
    const calls = mockEditor();
    render(<ProfilesScreen onImport={() => {}} />);
    await userEvent.click(await screen.findByRole('button', { name: 'New' }));
    await userEvent.click(screen.getByRole('button', { name: /FortiGate/ }));

    expect(await screen.findByRole('heading', { name: 'New profile' })).toBeInTheDocument();
    expect(screen.getByLabelText('Username and password (XAuth)')).toBeChecked();
    expect(screen.queryByText('enter a gateway')).not.toBeInTheDocument();
    // Weak (aggressive + PSK): said, not blocked.
    expect(await screen.findByText('Aggressive mode with a PSK…')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('enter a gateway')).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Fix the highlighted fields');
    expect(calls.some((c) => c.cmd === 'save_profile_draft')).toBe(false);

    await userEvent.type(screen.getByLabelText('Name'), 'Escritório');
    await userEvent.type(screen.getByLabelText('Identifier'), 'escritorio');
    await userEvent.type(screen.getByLabelText('Gateway'), 'vpn.example.com');
    await userEvent.type(screen.getByLabelText('Networks through the VPN'), '192.0.2.0/24');
    await userEvent.click(screen.getByLabelText('The password is a one-time code'));
    expect(await screen.findByText(/format 2/)).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText('enter a gateway')).not.toBeInTheDocument());

    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(lastDraft(calls, 'save_profile_draft')).toBeDefined());
    expect(lastDraft(calls, 'save_profile_draft')).toMatchObject({
      name: 'Escritório',
      id: 'escritorio',
      gateway: 'vpn.example.com',
      remoteTs: ['192.0.2.0/24'],
      xauth: true,
      otp: true,
    });
    expect(await screen.findByRole('heading', { name: 'Profiles' })).toBeInTheDocument();
  });

  it('edits a saved profile: fixed id, IKEv2 drops XAuth and picks its presets', async () => {
    const calls = mockEditor({ profiles: [saved], broken: [] });
    render(<ProfilesScreen onImport={() => {}} />);
    await userEvent.click(await screen.findByRole('button', { name: 'Edit Escritório' }));

    expect(await screen.findByRole('heading', { name: 'Edit Escritório' })).toBeInTheDocument();
    expect(screen.getByLabelText('Identifier')).toBeDisabled();
    expect(await screen.findByText('AES and SHA-256 with DH 2048')).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText('IKE version'));
    await userEvent.click(await screen.findByRole('option', { name: 'IKEv2' }));
    expect(screen.queryByLabelText('Username and password (XAuth)')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('Mode')).not.toBeInTheDocument();
    await waitFor(() =>
      expect(lastDraft(calls, 'check_profile_draft')).toMatchObject({
        ikeVersion: 2,
        aggressive: false,
        xauth: false,
        // Compatible exists in IKEv2 too: its IKEv2 proposals.
        espProposals: ['aes256-sha256-modp2048', 'aes256-sha256'],
      }),
    );

    await userEvent.click(screen.getByLabelText('Proposals'));
    await userEvent.click(await screen.findByRole('option', { name: 'Advanced' }));
    const phase1 = screen.getByLabelText('Phase 1 (IKE)');
    await userEvent.clear(phase1);
    await userEvent.type(phase1, 'aes128-sha256-ecp256');
    await waitFor(() =>
      expect(lastDraft(calls, 'check_profile_draft')?.ikeProposals).toEqual([
        'aes128-sha256-ecp256',
      ]),
    );

    await userEvent.click(screen.getByRole('button', { name: 'Back to the profiles' }));
    expect(await screen.findByRole('heading', { name: 'Profiles' })).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'save_profile_draft')).toBe(false);
  });

  it('IKEv2 with EAP asks for the gateway name and its CA, pasted or from a file', async () => {
    const calls = mockEditor();
    render(<ProfilesScreen onImport={() => {}} />);
    await userEvent.click(await screen.findByRole('button', { name: 'New' }));
    await userEvent.click(screen.getByRole('button', { name: /IKEv2 with username and password/ }));

    expect(await screen.findByLabelText("Name in the gateway's certificate")).toBeInTheDocument();
    expect(screen.queryByLabelText('Username and password (XAuth)')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Choose file…' })).toBeInTheDocument();

    await userEvent.type(
      screen.getByLabelText("Name in the gateway's certificate"),
      'vpn.example.com',
    );
    await userEvent.click(screen.getByLabelText('Gateway CA'));
    await userEvent.paste('-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----');
    await userEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(await screen.findByText(/IPsec Client Test CA/)).toBeInTheDocument();
    expect(screen.getByText(/valid until 2076-11-04/)).toBeInTheDocument();
    await waitFor(() =>
      expect(lastDraft(calls, 'check_profile_draft')).toMatchObject({
        eap: true,
        remoteId: 'vpn.example.com',
        ca: ['-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n'],
      }),
    );

    // Back to a PSK: the certificate fields go away with the CAs.
    await userEvent.click(screen.getByLabelText('How you sign in'));
    await userEvent.click(await screen.findByRole('option', { name: 'Pre-shared key (PSK)' }));
    expect(screen.queryByRole('button', { name: 'Choose file…' })).not.toBeInTheDocument();
    await waitFor(() =>
      expect(lastDraft(calls, 'check_profile_draft')).toMatchObject({ eap: false, ca: [] }),
    );
  });

  it('offers the five templates', async () => {
    mockEditor();
    render(<ProfilesScreen onImport={() => {}} />);
    await userEvent.click(await screen.findByRole('button', { name: 'New' }));
    const dialog = await screen.findByRole('dialog');
    for (const name of [
      /FortiGate/,
      /Cisco Easy VPN/,
      /IKEv2 with a PSK/,
      /IKEv2 with username and password/,
      /Blank/,
    ]) {
      expect(within(dialog).getByRole('button', { name })).toBeInTheDocument();
    }
  });
});
