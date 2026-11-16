import { FileUp, Network } from 'lucide-react';
import { useState } from 'react';
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
import { SetupBanner } from '../component/SetupBanner';
import { useComponent } from '../component/useComponent';
import { ConnectButton } from './ConnectButton';
import { CredentialsDialog } from './CredentialsDialog';
import { ErrorBanner, isCredentialError } from './ErrorBanner';
import { LogPanel } from './LogPanel';
import { StatusDetails } from './StatusDetails';
import { useConnection } from './useConnection';
import { useProfiles } from './useProfiles';

interface DialogState {
  fields: CredentialField[];
  keyringAvailable: boolean;
}

/** Tela principal: perfil, botão de conexão, estado, erros e log. */
export function HomeScreen({ onImport }: { onImport?: () => void }) {
  const { list, error: listError, selected, select } = useProfiles();
  const { state, connect, disconnect, dismissError } = useConnection();
  const component = useComponent();
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const [localError, setLocalError] = useState<UiError | null>(null);

  const error = localError ?? state.error ?? listError;
  const idle = state.phase === 'idle';
  const activeProfile = state.connection?.profileId;

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

  const setup =
    idle && component.info ? (
      <SetupBanner
        info={component.info}
        busy={component.busy === 'install'}
        onInstall={() => void installComponent()}
      />
    ) : null;

  if (list && list.profiles.length === 0) {
    return (
      <div className="flex w-full flex-col gap-4">
        {setup}
        <Card className="w-full">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <FileUp className="size-5 text-primary" aria-hidden />
              Nenhum perfil ainda
            </CardTitle>
            <CardDescription>
              Para conectar, importe o perfil (.json) que você recebeu da sua equipe, ou o
              swanctl.conf de uma VPN que já funciona.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Button onClick={onImport}>
              <FileUp />
              Importar perfil
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  return (
    <div className="flex w-full flex-col items-center gap-6">
      {setup}
      <div className="w-full">
        <Select
          value={selected?.id ?? ''}
          onValueChange={select}
          disabled={!idle || !list || list.profiles.length === 0}
        >
          <SelectTrigger aria-label="Perfil de conexão">
            <span className="flex min-w-0 items-center gap-2">
              <Network className="size-4 shrink-0 text-muted-foreground" aria-hidden />
              <SelectValue placeholder="Escolha um perfil" />
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
        {!idle && activeProfile && activeProfile !== selected?.id && (
          <p className="mt-2 text-xs text-muted-foreground">
            Conexão ativa: perfil {activeProfile}
          </p>
        )}
      </div>

      <ConnectButton
        phase={state.phase}
        disabled={idle && !selected}
        onConnect={() => void startConnect()}
        onDisconnect={() => void disconnect()}
      />

      <StatusDetails
        phase={state.phase}
        step={state.step}
        connection={state.connection}
        building={state.building}
      />

      {error && (
        <ErrorBanner
          error={error}
          onDismiss={dismiss}
          onFixCredentials={isCredentialError(error) ? () => void startConnect(true) : undefined}
          onInstallComponent={
            component.info?.canInstall ? () => void installComponent() : undefined
          }
          installing={component.busy === 'install'}
        />
      )}

      <LogPanel lines={state.logs} anonymize={anonymizeLog} />

      {selected && dialog && (
        <CredentialsDialog
          open
          onOpenChange={(open) => !open && setDialog(null)}
          profileName={selected.name}
          fields={dialog.fields}
          keyringAvailable={dialog.keyringAvailable}
          onSubmit={(input, remember) => void submitCredentials(input, remember)}
        />
      )}
    </div>
  );
}
