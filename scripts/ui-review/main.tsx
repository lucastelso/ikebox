// Fixtures for the UI review: every value is fake (RFC 5737 addresses,
// RFC 2606 names). The page takes ?scenario=&lang=&theme= from the URL.
import { emit } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from '@/App';
import { I18nProvider } from '@/i18n';
import './review.css';

const q = new URLSearchParams(location.search);
const scenario = q.get('scenario') ?? 'idle';
const lang = q.get('lang') === 'pt' ? 'pt' : 'en';
const theme = q.get('theme') === 'dark' ? 'dark' : 'light';
const pt = lang === 'pt';
const L = (en: string, ptText: string) => (pt ? ptText : en);

localStorage.setItem('ipsec-client:language', lang);
document.documentElement.setAttribute('data-theme', theme);
document.documentElement.lang = lang;

const profiles = [
  {
    id: 'example-corp',
    name: 'Example Corp',
    gateway: 'vpn.example.com',
    usesXauth: true,
    otp: false,
    remoteTs: ['192.0.2.0/24', '198.51.100.0/24'],
    security: 'weak' as const,
  },
  {
    id: 'lab-long-name',
    name: L(
      'Test laboratory with a rather long profile name',
      'Laboratório de testes com um nome de perfil bem comprido',
    ),
    gateway: '203.0.113.10',
    usesXauth: true,
    otp: false,
    remoteTs: ['203.0.113.0/24'],
    security: 'good' as const,
  },
];

const connection = {
  profileId: 'example-corp',
  established: true,
  gateway: 'vpn.example.com',
  vip: '198.51.100.10',
  remoteTs: ['192.0.2.0/24', '198.51.100.0/24'],
  establishedSecs: 3725,
  bytesIn: 48_234_112,
  bytesOut: 5_123_400,
};

const component = (state: string) => ({
  state,
  installedVersion: state === 'notInstalled' ? null : state === 'outdated' ? '0.2.0' : '0.3.0',
  installedStrongswan: state === 'notInstalled' ? null : '6.1.0',
  bundledVersion: '0.3.0',
  bundledStrongswan: '6.1.0',
  managedByPackage: false,
  canInstall: true,
  canUninstall: state !== 'notInstalled',
  problem: null,
});

const checks = [
  { name: L('User', 'Usuário'), level: 'ok', detail: L('regular (not root)', 'comum (não root)'), hint: null },
  {
    name: L('Network component', 'Componente de rede'),
    level: 'ok',
    detail: L('installed (embedded engine, helper 0.3.0)', 'instalado (engine embutido, helper 0.3.0)'),
    hint: null,
  },
  { name: L('xfrm_interface module', 'Módulo xfrm_interface'), level: 'ok', detail: L('loaded', 'carregado'), hint: null },
  { name: L('ip command (iproute2)', 'Comando ip (iproute2)'), level: 'ok', detail: L('available', 'disponível'), hint: null },
  {
    name: 'systemd-resolved',
    level: 'warn',
    detail: L('not running', 'não está em execução'),
    hint: L(
      'without systemd-resolved, the VPN DNS (internal domains) is not configured; the tunnel works by IP. Debian and openSUSE do not use it by default: install the systemd-resolved package and enable it',
      'sem o systemd-resolved, o DNS da VPN (domínios internos) não é configurado; o túnel funciona por IP. O Debian e o openSUSE não o usam por padrão: instale o pacote systemd-resolved e ative-o',
    ),
  },
  { name: L('System D-Bus', 'D-Bus do sistema'), level: 'ok', detail: L('available', 'disponível'), hint: null },
  { name: L('UDP ports 500/4500', 'Portas UDP 500/4500'), level: 'ok', detail: L('free', 'livres'), hint: null },
  {
    name: L('System keyring', 'Chaveiro do sistema'),
    level: 'ok',
    detail: L('available (credentials can be remembered)', 'disponível (credenciais podem ser lembradas)'),
    hint: null,
  },
];

const negotiationError = {
  kind: 'negotiation',
  message: L(
    'The gateway rejected phase 1 authentication: check the group PSK and the group id (auth.local_id; on FortiGate/Cisco it is usually keyid:<group-name>)',
    'O gateway recusou a autenticação da fase 1: confira a PSK do grupo e o id do grupo (auth.local_id; em FortiGate/Cisco costuma ser keyid:<nome-do-grupo>)',
  ),
  details: ['10[IKE] calculated HASH does not match HASH payload', '10[ENC] invalid HASH_V1 payload length, decryption failed?'],
};

const logLines = [
  '09[IKE] initiating Aggressive Mode IKE_SA example-corp[1] to <gateway>',
  '06[IKE] received XAuth vendor ID',
  '07[IKE] XAuth authentication of \'us***er\' (myself) successful',
  '08[IKE] IKE_SA example-corp[1] established',
  '15[IKE] CHILD_SA rede{1} established with SPIs and TS 198.51.100.10/32 === 192.0.2.0/24',
];

const editorPresets = [
  { preset: 'compatible', ike: ['aes256-sha256-modp2048', 'aes128-sha256-modp2048'], esp: ['aes256-sha256-modp2048', 'aes128-sha256-modp2048'], security: 'good' },
  { preset: 'legacy', ike: ['aes256-sha1-modp1024'], esp: ['aes256-sha1-modp1024'], security: 'weak' },
];

const editorDraft = {
  originalId: null,
  template: 'fortigate',
  id: scenario === 'editor-problems' ? '' : 'example-corp',
  name: scenario === 'editor-problems' ? '' : 'Example Corp',
  gateway: scenario === 'editor-problems' ? '' : 'vpn.example.com',
  ikeVersion: scenario === 'editor-eap' ? 2 : 1,
  aggressive: scenario !== 'editor-eap',
  encap: true,
  ikeProposals: editorPresets[0]?.ike ?? [],
  espProposals: editorPresets[0]?.esp ?? [],
  xauth: scenario !== 'editor-eap',
  otp: false,
  eap: scenario === 'editor-eap',
  ca: scenario === 'editor-eap' ? ['-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n'] : [],
  localId: scenario === 'editor-problems' || scenario === 'editor-eap' ? '' : 'keyid:example-group',
  remoteId: scenario === 'editor-eap' ? 'vpn.example.com' : '',
  remoteTs: scenario === 'editor-problems' ? [] : ['192.0.2.0/24'],
  requestVip: true,
  dnsServers: [],
  dnsDomains: scenario === 'editor-problems' ? [] : ['~corp.example.com'],
  useReceivedDns: true,
  dnsDefaultRoute: false,
  mtu: 1400,
  dpdDelaySecs: 30,
};

const hasProfiles = scenario !== 'first-use';
const componentState =
  scenario === 'first-use' ? 'notInstalled' : scenario === 'outdated' ? 'outdated' : 'ready';
let connected = scenario === 'connected';

mockIPC(
  (cmd) => {
    switch (cmd) {
      case 'list_profiles':
        return { profiles: hasProfiles ? profiles : [], broken: [] };
      case 'status':
        return connected ? connection : null;
      case 'component_status':
        return component(componentState);
      case 'run_diagnostics':
        return checks;
      case 'app_info':
        return { version: '0.3.0', engineImage: L('embedded', 'embutido') };
      case 'credentials_status':
        return scenario === 'credentials'
          ? { required: ['psk', 'xauthUser', 'xauthPassword'], stored: [], maskedUser: null, keyringAvailable: true }
          : { required: ['psk', 'xauthUser', 'xauthPassword'], stored: ['psk', 'xauthUser', 'xauthPassword'], maskedUser: 'us***er', keyringAvailable: true };
      case 'connect':
        if (scenario === 'error') throw negotiationError;
        // "connecting": never answers, the screen stays in progress.
        return new Promise(() => {});
      case 'profile_details':
        return {
          summary: profiles[0],
          ike: 'IKEv1 aggressive',
          ikeProposals: ['aes256-sha1-modp1024'],
          espProposals: ['aes256-sha1-modp1024'],
          auth: 'PSK + XAuth',
          localId: 'keyid:example-group',
          dnsServers: ['192.0.2.53'],
          dnsDomains: ['~corp.example.com'],
          useReceivedDns: true,
          mtu: 1400,
          securityNotes: [
            {
              level: 'weak',
              message: L(
                'Aggressive mode with a PSK: the gateway answers anyone who knows the group id with a hash of the PSK, which can be guessed offline.',
                'Modo agressivo com PSK: o gateway responde a quem souber o id do grupo com um hash da PSK, que pode ser adivinhada offline.',
              ),
            },
            {
              level: 'legacy',
              message: L(
                'IKEv1 is obsolete (RFC 9395): use IKEv2 when the gateway offers it.',
                'O IKEv1 está obsoleto (RFC 9395): use IKEv2 quando o gateway oferecer.',
              ),
            },
          ],
        };
      case 'new_profile_draft':
        return editorDraft;
      case 'proposal_presets':
        return editorPresets;
      case 'check_profile_draft':
        return {
          issues:
            scenario === 'editor-problems'
              ? [
                  { field: 'gateway', message: L('enter a valid DNS name or IP address', 'informe um nome DNS ou um endereço IP válido') },
                  { field: 'remoteTs', message: L('enter at least one remote network (e.g. 192.0.2.0/24)', 'informe ao menos uma rede remota (ex.: 192.0.2.0/24)') },
                ]
              : [],
          security: scenario === 'editor-eap' ? 'good' : 'weak',
          notes: [
            {
              level: 'weak',
              message: L(
                'Aggressive mode with a PSK: the gateway answers anyone who knows the group id with a hash of the PSK, which can be guessed offline.',
                'Modo agressivo com PSK: o gateway responde a quem souber o id do grupo com um hash da PSK, que pode ser adivinhada offline.',
              ),
            },
          ],
          preset: 'compatible',
          schema: scenario === 'editor-eap' ? 2 : 1,
          cas:
            scenario === 'editor-eap'
              ? [{ subject: 'CN=Example Corp Root CA,O=Example Corp', notAfter: '2036-01-31', key: 'RSA' }]
              : [],
        };
      case 'set_language':
        return null;
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

createRoot(document.getElementById('root') as HTMLElement).render(
  <StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </StrictMode>,
);

// Clicks the scenario needs, then lets the page finish loading (serve.py
// holds the load event until this script calls /__release).
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const click = (selector: string) => (document.querySelector(selector) as HTMLElement | null)?.click();
const navTo = (index: number) =>
  (document.querySelectorAll('nav button')[index] as HTMLElement | undefined)?.click();

async function play() {
  await sleep(250);
  if (scenario === 'credentials' || scenario === 'error' || scenario === 'connecting') {
    click('section.hero > div > button');
    if (scenario === 'connecting') {
      await sleep(100);
      await emit('session-update', { kind: 'preparing', step: L('starting the engine', 'subindo o engine') });
    }
  }
  if (scenario === 'connected') {
    for (const line of logLines) await emit('session-update', { kind: 'log', line });
    await sleep(50);
    (document.querySelector('main details') as HTMLDetailsElement | null)?.setAttribute('open', '');
  }
  if (scenario === 'profiles') navTo(1);
  if (scenario === 'profile-open') {
    navTo(1);
    await sleep(150);
    click('main button[aria-expanded]');
  }
  if (scenario === 'system') navTo(2);
  if (scenario === 'editor' || scenario === 'editor-problems' || scenario === 'editor-eap') {
    navTo(1);
    await sleep(150);
    click('main h2 + div button');
    await sleep(150);
    click('[role="dialog"] button');
    await sleep(300);
    if (scenario === 'editor-eap') {
      // The CA section is below the fold: bring it to the middle.
      document.querySelector('main textarea')?.scrollIntoView({ block: 'center' });
    }
    if (scenario === 'editor-problems') {
      const save = [...document.querySelectorAll('main button')].find(
        (b) => b.textContent === L('Save', 'Salvar'),
      ) as HTMLElement | undefined;
      save?.click();
    }
  }
  await sleep(400);
  connected = scenario === 'connected';
  await fetch('/__release').catch(() => {});
}
void play();
