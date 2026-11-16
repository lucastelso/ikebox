import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { type ComponentInfo, commands, type UiError } from '@/bindings';
import { useT } from '@/i18n';
import type { Messages } from '@/i18n/messages';

export type ComponentAction = 'install' | 'uninstall';

function report(error: UiError, action: ComponentAction, t: Messages['component']) {
  if (error.kind === 'cancelled') {
    toast.info(action === 'install' ? t.toast.installCancelled : t.toast.removeCancelled);
    return;
  }
  const tail = error.details.slice(-3).join('\n');
  toast.error(error.message, tail ? { description: tail } : undefined);
}

/** Network component state and the install/remove actions. */
export function useComponent() {
  const t = useT().component;
  const [info, setInfo] = useState<ComponentInfo | null>(null);
  const [busy, setBusy] = useState<ComponentAction | null>(null);

  const refresh = useCallback(async () => {
    const result = await commands.componentStatus();
    if (result.status === 'ok') setInfo(result.data ?? null);
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = useCallback(
    async (action: ComponentAction): Promise<boolean> => {
      setBusy(action);
      try {
        const result =
          action === 'install'
            ? await commands.installComponent()
            : await commands.uninstallComponent();
        if (result.status === 'error') {
          report(result.error, action, t);
          return false;
        }
        setInfo(result.data);
        if (action === 'install') {
          toast.success(t.toast.installed, { description: t.toast.installedHint });
        } else {
          toast.success(t.toast.removed);
        }
        return true;
      } finally {
        setBusy(null);
      }
    },
    [t],
  );

  return {
    info,
    busy,
    refresh,
    install: useCallback(() => run('install'), [run]),
    uninstall: useCallback(() => run('uninstall'), [run]),
  };
}

/** Install button text for the current state. */
export function installLabel(info: ComponentInfo, t: Messages['component']): string {
  switch (info.state) {
    case 'outdated':
      return t.update;
    case 'broken':
      return t.reinstall;
    default:
      return t.install;
  }
}
