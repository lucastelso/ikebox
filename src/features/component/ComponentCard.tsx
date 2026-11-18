import {
  CircleArrowUp,
  CircleCheck,
  CircleDashed,
  CircleX,
  LoaderCircle,
  ShieldCheck,
  Trash2,
} from 'lucide-react';
import { useState } from 'react';
import type { ComponentInfo, ComponentState } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useT } from '@/i18n';
import { cn } from '@/lib/utils';
import { type ComponentAction, installLabel } from './useComponent';

const BADGE: Record<ComponentState, { icon: typeof CircleCheck; tone: string }> = {
  notInstalled: { icon: CircleDashed, tone: 'text-muted-foreground' },
  ready: { icon: CircleCheck, tone: 'text-success' },
  outdated: { icon: CircleArrowUp, tone: 'text-warning' },
  broken: { icon: CircleX, tone: 'text-destructive' },
};

function versions(app: string | null, strongswan: string | null): string {
  return `Ikebox ${app ?? '?'} · strongSwan ${strongswan ?? '?'}`;
}

/** Network component on the System screen: state, versions, install and remove. */
export function ComponentCard({
  info,
  busy,
  onInstall,
  onUninstall,
}: {
  info: ComponentInfo | null;
  busy: ComponentAction | null;
  onInstall: () => void;
  onUninstall: () => void;
}) {
  const t = useT().component;
  const [confirming, setConfirming] = useState(false);
  const badge = info ? BADGE[info.state] : null;
  const showInstall = info?.canInstall && info.state !== 'ready';

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-2">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2">
            <ShieldCheck className="size-5 text-primary" aria-hidden />
            {t.title}
          </CardTitle>
          <CardDescription>{info ? t.description[info.state] : t.checking}</CardDescription>
        </div>
        {badge && (
          <span className={cn('flex shrink-0 items-center gap-1 text-xs font-medium', badge.tone)}>
            <badge.icon className="size-4" aria-hidden />
            {info && t.badge[info.state]}
          </span>
        )}
      </CardHeader>
      {info && (
        <CardContent className="flex flex-col gap-3 text-sm">
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs" data-selectable>
            {info.state !== 'notInstalled' && (
              <>
                <dt className="text-muted-foreground">{t.installed}</dt>
                <dd>{versions(info.installedVersion, info.installedStrongswan)}</dd>
              </>
            )}
            {info.bundledVersion && (
              <>
                <dt className="text-muted-foreground">{t.bundled}</dt>
                <dd>{versions(info.bundledVersion, info.bundledStrongswan)}</dd>
              </>
            )}
          </dl>
          {info.problem && (
            <p className="break-words text-xs text-destructive" data-selectable>
              {info.problem}
            </p>
          )}
          {info.managedByPackage && (
            <p className="text-xs text-muted-foreground">{t.managedByPackage}</p>
          )}
          {!info.bundledVersion && info.state === 'notInstalled' && (
            <p className="text-xs text-muted-foreground">{t.devBuild}</p>
          )}
          {(showInstall || info.canUninstall) && (
            <div className="flex flex-wrap items-center gap-2">
              {showInstall && (
                <Button size="sm" onClick={onInstall} disabled={busy !== null}>
                  {busy === 'install' && <LoaderCircle className="animate-spin" />}
                  {busy === 'install' ? t.installing : installLabel(info, t)}
                </Button>
              )}
              {info.canUninstall && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setConfirming(true)}
                  disabled={busy !== null}
                >
                  {busy === 'uninstall' ? <LoaderCircle className="animate-spin" /> : <Trash2 />}
                  {busy === 'uninstall' ? t.removing : t.remove}
                </Button>
              )}
            </div>
          )}
          {showInstall && <p className="text-xs text-muted-foreground">{t.passwordHint}</p>}
        </CardContent>
      )}
      {confirming && (
        <Dialog open onOpenChange={(open) => !open && setConfirming(false)}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{t.removeTitle}</DialogTitle>
              <DialogDescription>{t.removeBody}</DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="ghost" onClick={() => setConfirming(false)}>
                {t.cancel}
              </Button>
              <Button
                variant="destructive"
                onClick={() => {
                  setConfirming(false);
                  onUninstall();
                }}
              >
                {t.remove}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </Card>
  );
}
