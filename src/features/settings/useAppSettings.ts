import { useCallback, useEffect, useState } from 'react';
import { type AppSettingsView, commands, events } from '@/bindings';

/**
 * The app settings kept by Rust (close to the tray, the optional update
 * notice), refreshed when the update check finds a newer version.
 */
export function useAppSettings() {
  const [settings, setSettings] = useState<AppSettingsView | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSettings(await commands.appSettings());
    } catch {
      setSettings(null);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void refresh();
    events.updateFound
      .listen(() => void refresh())
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh]);

  const setCloseToTray = useCallback(async (enabled: boolean) => {
    setSettings(await commands.setCloseToTray(enabled));
  }, []);

  const setUpdateCheck = useCallback(async (enabled: boolean) => {
    setSettings(await commands.setUpdateCheck(enabled));
  }, []);

  return { settings, setCloseToTray, setUpdateCheck };
}
