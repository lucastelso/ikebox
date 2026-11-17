import { ArrowDown, ArrowUp } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Connection } from '@/bindings';
import { useLanguage, useT } from '@/i18n';
import { formatBytes, formatDuration } from '@/lib/format';

function Tile({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="rounded-2xl border bg-card px-4 py-3">
      <dt className="flex items-center gap-1 text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 truncate text-lg font-bold tabular-nums">{children}</dd>
    </div>
  );
}

/** The active connection in big numbers: virtual IP, time, traffic, networks. */
export function StatusDetails({ connection }: { connection: Connection }) {
  const t = useT().status;
  const { lang } = useLanguage();
  return (
    <div className="flex w-full flex-col gap-3" data-selectable>
      <dl className="grid grid-cols-2 gap-3">
        <Tile label={t.virtualIp}>
          {/* An address is never cut: it wraps (an IPv6 one is long). */}
          <span className="block whitespace-normal break-all font-mono text-sm leading-snug">
            {connection.vip ?? '—'}
          </span>
        </Tile>
        <Tile label={t.connectedFor}>
          {connection.establishedSecs != null ? formatDuration(connection.establishedSecs) : '—'}
        </Tile>
        <Tile
          label={
            <>
              <ArrowDown className="size-3.5 text-success" aria-hidden />
              {t.receivedTile}
            </>
          }
        >
          {formatBytes(connection.bytesIn ?? 0, lang)}
        </Tile>
        <Tile
          label={
            <>
              <ArrowUp className="size-3.5 text-primary" aria-hidden />
              {t.sentTile}
            </>
          }
        >
          {formatBytes(connection.bytesOut ?? 0, lang)}
        </Tile>
      </dl>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 rounded-2xl border bg-card px-4 py-3 text-sm">
        <dt className="text-muted-foreground">{t.networks}</dt>
        <dd className="break-words font-mono text-[13px]">
          {connection.remoteTs.join(', ') || '—'}
        </dd>
        <dt className="text-muted-foreground">{t.gateway}</dt>
        <dd className="truncate font-mono text-[13px]">{connection.gateway || '—'}</dd>
      </dl>
    </div>
  );
}
