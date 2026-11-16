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
import { cn } from '@/lib/utils';
import { type ComponentAction, installLabel } from './useComponent';

const DESCRIPTION: Record<ComponentState, string> = {
  notInstalled:
    'Cria o túnel da VPN no sistema. É instalado uma vez, com a senha de administrador; depois, conectar não pede senha.',
  ready: 'Instalado e funcionando.',
  outdated: 'Este app traz uma versão diferente do componente.',
  broken: 'Instalado, mas não está respondendo.',
};

const BADGE: Record<ComponentState, { icon: typeof CircleCheck; tone: string; label: string }> = {
  notInstalled: { icon: CircleDashed, tone: 'text-muted-foreground', label: 'Não instalado' },
  ready: { icon: CircleCheck, tone: 'text-success', label: 'Instalado' },
  outdated: { icon: CircleArrowUp, tone: 'text-warning', label: 'Atualização disponível' },
  broken: { icon: CircleX, tone: 'text-destructive', label: 'Com problema' },
};

function versions(app: string | null, strongswan: string | null): string {
  return `IPsec Client ${app ?? '?'} · strongSwan ${strongswan ?? '?'}`;
}

/** Componente de rede na tela Sistema: estado, versões, instalar e remover. */
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
  const [confirming, setConfirming] = useState(false);
  const badge = info ? BADGE[info.state] : null;
  const showInstall = info?.canInstall && info.state !== 'ready';

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-2">
        <div className="grid gap-1.5">
          <CardTitle className="flex items-center gap-2">
            <ShieldCheck className="size-5 text-primary" aria-hidden />
            Componente de rede
          </CardTitle>
          <CardDescription>{info ? DESCRIPTION[info.state] : 'Verificando…'}</CardDescription>
        </div>
        {badge && (
          <span className={cn('flex shrink-0 items-center gap-1 text-xs font-medium', badge.tone)}>
            <badge.icon className="size-4" aria-hidden />
            {badge.label}
          </span>
        )}
      </CardHeader>
      {info && (
        <CardContent className="flex flex-col gap-3 text-sm">
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs" data-selectable>
            {info.state !== 'notInstalled' && (
              <>
                <dt className="text-muted-foreground">Instalado</dt>
                <dd>{versions(info.installedVersion, info.installedStrongswan)}</dd>
              </>
            )}
            {info.bundledVersion && (
              <>
                <dt className="text-muted-foreground">Neste app</dt>
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
            <p className="text-xs text-muted-foreground">
              Instalado pelo pacote do sistema (.rpm/.deb): atualize ou remova pelo gerenciador de
              pacotes.
            </p>
          )}
          {!info.bundledVersion && info.state === 'notInstalled' && (
            <p className="text-xs text-muted-foreground">
              Esta versão do app não traz o componente (build de desenvolvimento).
            </p>
          )}
          {(showInstall || info.canUninstall) && (
            <div className="flex flex-wrap items-center gap-2">
              {showInstall && (
                <Button size="sm" onClick={onInstall} disabled={busy !== null}>
                  {busy === 'install' && <LoaderCircle className="animate-spin" />}
                  {busy === 'install' ? 'Instalando…' : installLabel(info)}
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
                  {busy === 'uninstall' ? 'Removendo…' : 'Remover'}
                </Button>
              )}
            </div>
          )}
          {showInstall && (
            <p className="text-xs text-muted-foreground">
              O sistema vai pedir a senha de administrador.
            </p>
          )}
        </CardContent>
      )}
      {confirming && (
        <Dialog open onOpenChange={(open) => !open && setConfirming(false)}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Remover o componente de rede?</DialogTitle>
              <DialogDescription>
                Sem ele, o IPsec Client não consegue conectar. Seus perfis e credenciais continuam
                guardados. O sistema vai pedir a senha de administrador.
              </DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="ghost" onClick={() => setConfirming(false)}>
                Cancelar
              </Button>
              <Button
                variant="destructive"
                onClick={() => {
                  setConfirming(false);
                  onUninstall();
                }}
              >
                Remover
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </Card>
  );
}
