import { ArrowDown, ArrowUp } from 'lucide-react';
import type { Connection } from '@/bindings';
import { Card, CardContent } from '@/components/ui/card';
import { formatBytes, formatDuration } from '@/lib/format';
import type { Phase } from './connectionReducer';

const HEADLINE: Record<Phase, string> = {
  idle: 'Desconectado',
  preparing: 'Preparando…',
  negotiating: 'Negociando com o gateway…',
  connected: 'Conectado',
  reconnecting: 'Reconectando…',
  disconnecting: 'Desconectando…',
};

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
  return (
    <div className="flex w-full flex-col items-center gap-4">
      <div className="text-center" aria-live="polite">
        <p className={`text-lg font-semibold ${TONE[phase]}`}>{HEADLINE[phase]}</p>
        {phase === 'preparing' && step && (
          <p className="text-sm text-muted-foreground">
            {step}
            {building && ' (primeiro uso: pode levar cerca de 1 minuto)'}
          </p>
        )}
      </div>

      {connection && (phase === 'connected' || phase === 'reconnecting') && (
        <Card className="w-full">
          <CardContent
            className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 pt-5 text-sm"
            data-selectable
          >
            <dt className="text-muted-foreground">IP virtual</dt>
            <dd className="font-mono">{connection.vip ?? '—'}</dd>
            <dt className="text-muted-foreground">Gateway</dt>
            <dd className="truncate font-mono">{connection.gateway || '—'}</dd>
            <dt className="text-muted-foreground">Redes</dt>
            <dd className="font-mono">{connection.remoteTs.join(', ') || '—'}</dd>
            {connection.establishedSecs != null && (
              <>
                <dt className="text-muted-foreground">Conectado há</dt>
                <dd>{formatDuration(connection.establishedSecs)}</dd>
              </>
            )}
            <dt className="text-muted-foreground">Tráfego</dt>
            <dd className="flex items-center gap-3">
              <span className="inline-flex items-center gap-1">
                <ArrowDown className="size-3.5 text-success" aria-label="recebido" />
                {formatBytes(connection.bytesIn ?? 0)}
              </span>
              <span className="inline-flex items-center gap-1">
                <ArrowUp className="size-3.5 text-primary" aria-label="enviado" />
                {formatBytes(connection.bytesOut ?? 0)}
              </span>
            </dd>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
