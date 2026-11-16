import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { type ComponentInfo, commands, type UiError } from '@/bindings';

export type ComponentAction = 'install' | 'uninstall';

function report(error: UiError, action: ComponentAction) {
  if (error.kind === 'cancelled') {
    toast.info(action === 'install' ? 'Instalação cancelada.' : 'Remoção cancelada.');
    return;
  }
  const tail = error.details.slice(-3).join('\n');
  toast.error(error.message, tail ? { description: tail } : undefined);
}

/** Estado do componente de rede e as ações de instalar/remover. */
export function useComponent() {
  const [info, setInfo] = useState<ComponentInfo | null>(null);
  const [busy, setBusy] = useState<ComponentAction | null>(null);

  const refresh = useCallback(async () => {
    const result = await commands.componentStatus();
    if (result.status === 'ok') setInfo(result.data ?? null);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = useCallback(async (action: ComponentAction): Promise<boolean> => {
    setBusy(action);
    try {
      const result =
        action === 'install'
          ? await commands.installComponent()
          : await commands.uninstallComponent();
      if (result.status === 'error') {
        report(result.error, action);
        return false;
      }
      setInfo(result.data);
      if (action === 'install') {
        toast.success('Componente de rede instalado', {
          description: 'Pronto: conectar não pede mais a senha de administrador.',
        });
      } else {
        toast.success('Componente de rede removido');
      }
      return true;
    } finally {
      setBusy(null);
    }
  }, []);

  return {
    info,
    busy,
    refresh,
    install: useCallback(() => run('install'), [run]),
    uninstall: useCallback(() => run('uninstall'), [run]),
  };
}

/** Texto do botão de instalação conforme a situação. */
export function installLabel(info: ComponentInfo): string {
  switch (info.state) {
    case 'outdated':
      return 'Atualizar';
    case 'broken':
      return 'Reinstalar';
    default:
      return 'Instalar';
  }
}
