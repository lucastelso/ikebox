import { ChevronDown, Download, Trash2 } from 'lucide-react';
import { useState } from 'react';
import { commands, type ProfileDetails, type ProfileSummary } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { useT } from '@/i18n';
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
  const t = useT().profiles;
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
          className="flex min-w-0 flex-1 items-center gap-2 rounded-lg text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
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
        <Button size="icon" variant="ghost" onClick={onExport} aria-label={t.export(profile.name)}>
          <Download />
        </Button>
        <Button size="icon" variant="ghost" onClick={onDelete} aria-label={t.delete(profile.name)}>
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
              <Row label={t.field.id} value={details.summary.id} />
              <Row label={t.field.protocol} value={details.ike} />
              <Row label={t.field.auth} value={details.auth} />
              <Row label={t.field.groupId} value={details.localId ?? undefined} />
              <Row label={t.field.networks} value={details.summary.remoteTs.join(', ')} />
              <Row label={t.field.phase1} value={details.ikeProposals.join(', ')} />
              <Row label={t.field.phase2} value={details.espProposals.join(', ')} />
              <Row label={t.field.dns} value={details.dnsServers.join(', ')} />
              <Row label={t.field.domains} value={details.dnsDomains.join(', ')} />
            </>
          ) : (
            <dd className="col-span-2 text-muted-foreground">{t.loading}</dd>
          )}
        </dl>
      )}
    </Card>
  );
}
