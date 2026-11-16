import { useCallback, useEffect, useState } from 'react';
import { commands, type ProfileList, type UiError } from '@/bindings';

const SELECTED_KEY = 'ipsec-client:perfil-selecionado';

function readSelected(): string | null {
  try {
    return localStorage.getItem(SELECTED_KEY);
  } catch {
    return null;
  }
}

function writeSelected(id: string): void {
  try {
    localStorage.setItem(SELECTED_KEY, id);
  } catch {
    // Conveniência apenas: sem armazenamento, o app só não lembra a escolha.
  }
}

/** Perfis disponíveis e o selecionado (lembrado entre execuções). */
export function useProfiles() {
  const [list, setList] = useState<ProfileList | null>(null);
  const [error, setError] = useState<UiError | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(readSelected);

  const reload = useCallback(async () => {
    const result = await commands.listProfiles();
    if (result.status === 'ok') {
      setList(result.data);
      setError(null);
      setSelectedId((current) => {
        const ids = result.data.profiles.map((p) => p.id);
        if (current && ids.includes(current)) return current;
        return ids[0] ?? null;
      });
    } else {
      setError(result.error);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const select = useCallback((id: string) => {
    setSelectedId(id);
    writeSelected(id);
  }, []);

  const selected = list?.profiles.find((p) => p.id === selectedId) ?? null;
  return { list, error, selected, select, reload };
}
