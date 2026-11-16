import { FileJson, FileText, TriangleAlert } from 'lucide-react';
import { type FormEvent, useEffect, useId, useState } from 'react';
import { commands, type ImportPreview, type ProfileSummary, type UiError } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

function splitList(value: string): string[] {
  return value
    .split(/[\s,;]+/)
    .map((v) => v.trim())
    .filter(Boolean);
}

/** Confirma a importação de um perfil (.json) ou swanctl.conf. */
export function ImportDialog({
  preview,
  onClose,
  onImported,
}: {
  preview: ImportPreview;
  onClose: () => void;
  onImported: (profile: ProfileSummary) => void;
}) {
  const id = useId();
  const [profileId, setProfileId] = useState(preview.suggestedId);
  const [name, setName] = useState(preview.suggestedName);
  const [overwrite, setOverwrite] = useState(false);
  const [dnsServers, setDnsServers] = useState('');
  const [dnsDomains, setDnsDomains] = useState('');
  const [useReceived, setUseReceived] = useState(false);
  const [error, setError] = useState<UiError | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setProfileId(preview.suggestedId);
    setName(preview.suggestedName);
  }, [preview]);

  const swanctl = preview.kind === 'swanctl';
  const conflict = preview.exists && profileId === preview.suggestedId;

  async function submit(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    const result = await commands.confirmImport(preview.token, {
      id: profileId,
      name,
      dnsServers: swanctl ? splitList(dnsServers) : [],
      dnsDomains: swanctl ? splitList(dnsDomains) : [],
      useReceivedDns: swanctl && useReceived,
      overwrite,
    });
    setSaving(false);
    if (result.status === 'ok') onImported(result.data);
    else setError(result.error);
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {swanctl ? (
                <FileText className="size-5 text-primary" aria-hidden />
              ) : (
                <FileJson className="size-5 text-primary" aria-hidden />
              )}
              Importar perfil
            </DialogTitle>
            <DialogDescription>
              {preview.fileName} · {swanctl ? 'swanctl.conf' : 'perfil do IPsec Client'}
            </DialogDescription>
          </DialogHeader>

          {preview.error ? (
            <p role="alert" className="rounded-md bg-destructive/10 p-3 text-sm text-destructive">
              {preview.error}
            </p>
          ) : (
            <>
              <div className="grid gap-1 rounded-lg bg-muted p-3 text-sm" data-selectable>
                <p>
                  <span className="text-muted-foreground">Gateway: </span>
                  <span className="font-mono">{preview.gateway}</span>
                </p>
                <p>
                  <span className="text-muted-foreground">Redes: </span>
                  <span className="font-mono">{preview.remoteTs.join(', ')}</span>
                </p>
                {swanctl && (
                  <p className="text-xs text-muted-foreground">
                    Os segredos do arquivo não são lidos: você informa as credenciais ao conectar.
                  </p>
                )}
              </div>

              <div className="grid gap-4">
                <div className="grid gap-2">
                  <Label htmlFor={`${id}-name`}>Nome</Label>
                  <Input id={`${id}-name`} value={name} onChange={(e) => setName(e.target.value)} />
                </div>
                <div className="grid gap-2">
                  <Label htmlFor={`${id}-id`}>Identificador</Label>
                  <Input
                    id={`${id}-id`}
                    value={profileId}
                    onChange={(e) => setProfileId(e.target.value.toLowerCase())}
                    spellCheck={false}
                  />
                  <p className="text-xs text-muted-foreground">
                    Letras minúsculas, números e hífen.
                  </p>
                </div>

                {swanctl && (
                  <fieldset className="grid gap-3 rounded-lg border p-3">
                    <legend className="px-1 text-sm font-medium">DNS da VPN (opcional)</legend>
                    <div className="grid gap-2">
                      <Label htmlFor={`${id}-dns`}>Servidores DNS</Label>
                      <Input
                        id={`${id}-dns`}
                        placeholder="ex.: 192.0.2.53"
                        value={dnsServers}
                        onChange={(e) => setDnsServers(e.target.value)}
                        spellCheck={false}
                      />
                    </div>
                    <div className="grid gap-2">
                      <Label htmlFor={`${id}-domains`}>Domínios internos</Label>
                      <Input
                        id={`${id}-domains`}
                        placeholder="ex.: ~corp.example.com"
                        value={dnsDomains}
                        onChange={(e) => setDnsDomains(e.target.value)}
                        spellCheck={false}
                      />
                      <p className="text-xs text-muted-foreground">
                        Com "~" na frente, só os nomes desse domínio usam o DNS da VPN.
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      <Checkbox
                        id={`${id}-received`}
                        checked={useReceived}
                        onCheckedChange={(c) => setUseReceived(c === true)}
                      />
                      <Label htmlFor={`${id}-received`}>
                        Usar também o DNS enviado pelo gateway
                      </Label>
                    </div>
                  </fieldset>
                )}

                {preview.warnings.length > 0 && (
                  <details className="text-xs text-muted-foreground">
                    <summary className="flex cursor-pointer items-center gap-1">
                      <TriangleAlert className="size-3.5 text-warning" aria-hidden />
                      {preview.warnings.length} aviso(s) da conversão
                    </summary>
                    <ul className="mt-2 list-disc pl-5" data-selectable>
                      {preview.warnings.map((w) => (
                        <li key={w}>{w}</li>
                      ))}
                    </ul>
                  </details>
                )}

                {conflict && (
                  <div className="flex items-start gap-2 rounded-md bg-warning/10 p-3">
                    <Checkbox
                      id={`${id}-overwrite`}
                      checked={overwrite}
                      onCheckedChange={(c) => setOverwrite(c === true)}
                    />
                    <Label htmlFor={`${id}-overwrite`} className="leading-snug">
                      Já existe um perfil "{preview.suggestedId}". Substituir? (ou mude o
                      identificador)
                    </Label>
                  </div>
                )}
              </div>
            </>
          )}

          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error.message}
            </p>
          )}

          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancelar
            </Button>
            {!preview.error && (
              <Button type="submit" disabled={saving || !profileId || (conflict && !overwrite)}>
                Importar
              </Button>
            )}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
