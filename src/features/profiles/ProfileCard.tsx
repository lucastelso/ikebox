import { ChevronDown, Download, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { commands, type ProfileDetails, type ProfileSummary } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { cn } from '@/lib/utils';

function Row({ label, value }: { label: string; value: string | undefined }) {
  if (!value) return null;
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="break-words font-mono">{value}</dd>
    </>
  );
}

/** Um perfil na lista: resumo, detalhes sob demanda, exportar e excluir. */
export function ProfileCard({
  profile,
  onExport,
  onDelete,
}: {
  profile: ProfileSummary;
  onExport: () => void;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [details, setDetails] = useState<ProfileDetails | null>(null);

  async function toggle() {
    const next = !open;
    setOpen(next);
    if (next && !details) {
      const result = await commands.profileDetails(profile.id);
      if (result.status === 'ok') setDetails(result.data);
    }
  }

  return (
    <Card className="w-full">
      <div className="flex items-center gap-2 p-4">
        <button
          type="button"
          onClick={() => void toggle()}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <ChevronDown
            className={cn(
              'size-4 shrink-0 text-muted-foreground transition-transform',
              open && 'rotate-180',
            )}
            aria-hidden
          />
          <span className="min-w-0">
            <span className="block truncate font-medium">{profile.name}</span>
            <span className="block truncate text-xs text-muted-foreground">{profile.gateway}</span>
          </span>
        </button>
        <Button
          size="icon"
          variant="ghost"
          onClick={onExport}
          aria-label={`Exportar ${profile.name}`}
        >
          <Download />
        </Button>
        <Button
          size="icon"
          variant="ghost"
          onClick={onDelete}
          aria-label={`Excluir ${profile.name}`}
        >
          <Trash2 className="text-destructive" />
        </Button>
      </div>
      {open && (
        <dl
          className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 border-t px-4 py-3 text-xs"
          data-selectable
        >
          {details ? (
            <>
              <Row label="Identificador" value={details.summary.id} />
              <Row label="Protocolo" value={details.ike} />
              <Row label="Autenticação" value={details.auth} />
              <Row label="Id do grupo" value={details.localId ?? undefined} />
              <Row label="Redes" value={details.summary.remoteTs.join(', ')} />
              <Row label="Fase 1" value={details.ikeProposals.join(', ')} />
              <Row label="Fase 2" value={details.espProposals.join(', ')} />
              <Row label="DNS" value={details.dnsServers.join(', ')} />
              <Row label="Domínios" value={details.dnsDomains.join(', ')} />
            </>
          ) : (
            <dd className="col-span-2 text-muted-foreground">Carregando…</dd>
          )}
        </dl>
      )}
    </Card>
  );
}
