import {
  FileUp,
  KeyRound,
  LoaderCircle,
  Network,
  Shield,
  ShieldAlert,
  ShieldCheck,
  X,
} from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';
import { type CredentialField, type CredentialsInput, commands, type UiError } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useT } from '@/i18n';
import { SetupBanner } from '../component/SetupBanner';
import { useComponent } from '../component/useComponent';
import { SecurityBadge } from '../profiles/SecurityBadge';
import { HeroAction } from './ConnectButton';
import { CredentialsDialog } from './CredentialsDialog';
import type { Phase } from './connectionReducer';
import { ErrorBanner, isComponentError, isCredentialError } from './ErrorBanner';
import { LogPanel } from './LogPanel';
import { StatusDetails } from './StatusDetails';
import { useConnection } from './useConnection';
import { useProfiles } from './useProfiles';

interface DialogState {
  fields: CredentialField[];
  keyringAvailable: boolean;
}

/** Color of the top of the screen (see `.hero` in index.css). */
type Tone = 'idle' | 'busy' | 'ok' | 'bad';

function toneOf(phase: Phase, failed: boolean): Tone {
  if (phase === 'connected') return 'ok';
  if (phase === 'idle') return failed ? 'bad' : 'idle';
  return 'busy';
}

const TONE_ICON: Record<Tone, typeof Shield> = {
  idle: Shield,
  busy: LoaderCircle,
  ok: ShieldCheck,
  bad: ShieldAlert,
};

/** Tela principal: o topo colorido pelo estado, a ação principal e os detalhes. */
export function HomeScreen({ onImport }: { onImport?: () => void }) {
  const t = useT();
  const { list, error: listError, selected, select } = useProfiles();
  const { state, connect, disconnect, dismissError } = useConnection();
  const component = useComponent();
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [localError, setLocalError] = useState<UiError | null>(null);
  // Days until the client certificate of the selected profile expires, when
  // it is 30 or fewer (ADR-0024); null otherwise.
  const [certDays, setCertDays] = useState<number | null>(null);
  const selectedId = selected?.id;
  useEffect(() => {
    setCertDays(null);
    if (!selectedId) return;
    let cancelled = false;
    void commands.credentialsStatus(selectedId).then((result) => {
      const cert = result.status === 'ok' ? result.data.clientCertificate : null;
      if (!cancelled && cert && cert.daysLeft <= 30) setCertDays(cert.daysLeft);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const error = localError ?? state.error ?? listError;
  const phase = state.phase;
  const idle = phase === 'idle';
  const activeProfile = state.connection?.profileId;
  const noProfiles = list !== null && list.profiles.length === 0;
  const tone = toneOf(phase, error !== null);

  async function startConnect(askAll = false) {
    if (!selected) return;
    setLocalError(null);
    dismissError();
    const status = await commands.credentialsStatus(selected.id);
    if (status.status === 'error') {
      setLocalError(status.error);
      return;
    }
    const { required, stored, keyringAvailable } = status.data;
    const missing = askAll ? required : required.filter((f) => !stored.includes(f));
    if (missing.length > 0) {
      setDialog({ fields: missing, keyringAvailable });
      return;
    }
    await connect(selected.id, {}, false);
  }

  async function submitCredentials(input: CredentialsInput, remember: boolean) {
    setDialog(null);
    if (selected) await connect(selected.id, input, remember);
  }

  function dismiss() {
    setLocalError(null);
    dismissError();
  }

  async function installComponent() {
    if (await component.install()) dismiss();
  }

  /** O log é da conexão ativa (ou do perfil escolhido, se ela já caiu). */
  async function anonymizeLog(lines: string[]): Promise<string | null> {
    const profileId = activeProfile ?? selected?.id;
    if (!profileId) return null;
    const result = await commands.anonymizeLog(profileId, lines);
    return result.status === 'ok' ? result.data : null;
  }

  const profileName = selected?.name ?? activeProfile ?? '';
  const headline = tone === 'bad' ? t.phase.failed : t.phase.headline[phase];
  let subtitle: string;
  if (tone === 'bad') subtitle = t.phase.failedSubtitle;
  else if (noProfiles && idle) subtitle = t.home.importToConnect;
  else if (phase === 'connected') subtitle = t.phase.subtitle.connected(profileName);
  else if (phase === 'preparing' && state.step)
    subtitle = state.step + (state.building ? t.status.firstRun : '');
  else subtitle = t.phase.subtitle[phase];

  /** The one thing to do now, in the colored top. */
  function action(): ReactNode {
    if (phase === 'preparing' || phase === 'negotiating') {
      return (
        <HeroAction
          label={t.phase.action[phase]}
          solid={false}
          icon={<X aria-hidden />}
          onClick={() => void disconnect()}
        />
      );
    }
    if (phase === 'connected' || phase === 'reconnecting') {
      return (
        <HeroAction label={t.phase.action[phase]} solid={false} onClick={() => void disconnect()} />
      );
    }
    if (phase === 'disconnecting') {
      return (
        <HeroAction
          label={t.phase.action.disconnecting}
          solid={false}
          busy
          disabled
          onClick={() => {}}
        />
      );
    }
    if (error && isCredentialError(error)) {
      return (
        <HeroAction
          label={t.error.enterCredentials}
          solid
          icon={<KeyRound aria-hidden />}
          onClick={() => void startConnect(true)}
        />
      );
    }
    if (error && isComponentError(error) && component.info?.canInstall) {
      const installing = component.busy === 'install';
      const label = installing
        ? t.error.installing
        : error.kind === 'componentOutdated'
          ? t.error.updateComponent
          : t.error.installComponent;
      return (
        <HeroAction
          label={label}
          solid
          busy={installing}
          disabled={installing}
          onClick={() => void installComponent()}
        />
      );
    }
    if (noProfiles) return null;
    return (
      <HeroAction
        label={t.phase.action.idle}
        solid
        disabled={!selected}
        onClick={() => void startConnect()}
      />
    );
  }

  const ToneIcon = TONE_ICON[tone];
  const setupNeeded =
    idle && component.info?.canInstall === true && component.info.state !== 'ready';
  const numbered = setupNeeded && noProfiles;

  return (
    <div className="flex w-full flex-col">
      <section
        className="hero rounded-b-[28px] px-5 pb-6 pt-4"
        data-state={tone}
        aria-labelledby="connection-state"
      >
        <div className="mx-auto flex max-w-md flex-col gap-4">
          <div className="flex items-center gap-2">
            <img src="/logo.svg" alt="" className="size-6" />
            <span className="text-sm font-bold">IPsec Client</span>
            {!noProfiles && (
              <div className="ml-auto min-w-0 max-w-[62%]">
                <Select value={selected?.id ?? ''} onValueChange={select} disabled={!idle || !list}>
                  <SelectTrigger
                    aria-label={t.home.profileSelect}
                    className="h-9 rounded-full border-0 bg-white/15 text-white focus-visible:ring-white disabled:opacity-80 [&_svg]:text-white"
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <Network className="size-4 shrink-0" aria-hidden />
                      <SelectValue placeholder={t.home.chooseProfile} />
                    </span>
                  </SelectTrigger>
                  <SelectContent>
                    {list?.profiles.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        {p.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </div>

          <div className="flex items-center gap-4" aria-live="polite">
            <div className="grid size-16 shrink-0 place-items-center rounded-[20px] bg-white/15">
              <ToneIcon
                className={tone === 'busy' ? 'size-8 animate-spin' : 'size-8'}
                aria-hidden
              />
            </div>
            <div className="min-w-0">
              <h1
                id="connection-state"
                className="text-[26px] font-extrabold leading-tight tracking-tight"
              >
                {headline}
              </h1>
              <p className="text-sm text-white/85">{subtitle}</p>
              {selected && selected.security !== 'good' && (
                <SecurityBadge level={selected.security} onHero className="mt-1.5" />
              )}
              {certDays !== null && (
                <p className="mt-1.5 w-fit rounded-full bg-white/15 px-2 py-0.5 text-xs font-semibold">
                  {certDays < 0 ? t.home.certExpired : t.home.certExpires(certDays)}
                </p>
              )}
            </div>
          </div>

          {action()}
        </div>
      </section>

      <div className="mx-auto flex w-full max-w-md flex-col gap-3 p-5">
        {setupNeeded && component.info && (
          <SetupBanner
            info={component.info}
            busy={component.busy === 'install'}
            onInstall={() => void installComponent()}
            step={numbered ? 1 : undefined}
          />
        )}

        {noProfiles && (
          <Card className="w-full">
            <CardHeader>
              {numbered && (
                <p className="text-xs font-semibold text-muted-foreground">{t.home.step(2)}</p>
              )}
              <CardTitle className="flex items-center gap-2">
                <FileUp className="size-5 text-primary" aria-hidden />
                {t.home.noProfilesTitle}
              </CardTitle>
              <CardDescription>{t.home.noProfilesBody}</CardDescription>
            </CardHeader>
            <CardContent>
              <Button onClick={onImport}>
                <FileUp />
                {t.home.importProfile}
              </Button>
            </CardContent>
          </Card>
        )}

        {error && <ErrorBanner error={error} onDismiss={dismiss} />}

        {state.connection && (phase === 'connected' || phase === 'reconnecting') && (
          <StatusDetails connection={state.connection} />
        )}

        {!idle && activeProfile && activeProfile !== selected?.id && (
          <p className="text-xs text-muted-foreground">{t.home.activeProfile(activeProfile)}</p>
        )}

        {!noProfiles && <LogPanel lines={state.logs} anonymize={anonymizeLog} />}
      </div>

      {selected && dialog && (
        <CredentialsDialog
          open
          onOpenChange={(open) => !open && setDialog(null)}
          profileId={selected.id}
          profileName={selected.name}
          fields={dialog.fields}
          groupPsk={selected.usesXauth}
          otp={selected.otp}
          keyringAvailable={dialog.keyringAvailable}
          onSubmit={(input, remember) => void submitCredentials(input, remember)}
        />
      )}
    </div>
  );
}
