import { ArrowDown, ArrowUp } from 'lucide-react';
import type { Connection } from '@/bindings';
import { Card, CardContent } from '@/components/ui/card';
import { useLanguage, useT } from '@/i18n';
import { formatBytes, formatDuration } from '@/lib/format';
import type { Phase } from './connectionReducer';

const TONE: Record<Phase, string> = {
  idle: 'text-muted-foreground',
  preparing: 'text-warning',
  negotiating: 'text-warning',
  connected: 'text-success',
  reconnecting: 'text-warning',
  disconnecting: 'text-muted-foreground',
};

/** Estado em uma linha + detalhes da conexão ativa. */
export function StatusDetails({
  phase,
  step,
  connection,
  building,
}: {
  phase: Phase;
  step: string | null;
  connection: Connection | null;
  building: boolean;
}) {
  const t = useT();
  const { lang } = useLanguage();
  return (
    <div className="flex w-full flex-col items-center gap-4">
      <div className="text-center" aria-live="polite">
        <p className={`text-lg font-semibold ${TONE[phase]}`}>{t.phase.headline[phase]}</p>
        {phase === 'preparing' && step && (
          <p className="text-sm text-muted-foreground">
            {step}
            {building && t.status.firstRun}
          </p>
        )}
      </div>

      {connection && (phase === 'connected' || phase === 'reconnecting') && (
        <Card className="w-full">
          <CardContent
            className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 pt-5 text-sm"
            data-selectable
          >
            <dt className="text-muted-foreground">{t.status.virtualIp}</dt>
            <dd className="font-mono">{connection.vip ?? '—'}</dd>
            <dt className="text-muted-foreground">{t.status.gateway}</dt>
            <dd className="truncate font-mono">{connection.gateway || '—'}</dd>
            <dt className="text-muted-foreground">{t.status.networks}</dt>
            <dd className="font-mono">{connection.remoteTs.join(', ') || '—'}</dd>
            {connection.establishedSecs != null && (
              <>
                <dt className="text-muted-foreground">{t.status.connectedFor}</dt>
                <dd>{formatDuration(connection.establishedSecs)}</dd>
              </>
            )}
            <dt className="text-muted-foreground">{t.status.traffic}</dt>
            <dd className="flex items-center gap-3">
              <span className="inline-flex items-center gap-1">
                <ArrowDown className="size-3.5 text-success" aria-label={t.status.received} />
                {formatBytes(connection.bytesIn ?? 0, lang)}
              </span>
              <span className="inline-flex items-center gap-1">
                <ArrowUp className="size-3.5 text-primary" aria-label={t.status.sent} />
                {formatBytes(connection.bytesOut ?? 0, lang)}
              </span>
            </dd>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
