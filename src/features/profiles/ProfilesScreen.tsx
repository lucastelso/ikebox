import { FileUp, TriangleAlert } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { commands, type ProfileList, type ProfileSummary } from '@/bindings';
import { Button } from '@/components/ui/button';
import { DeleteDialog } from './DeleteDialog';
import { ProfileCard } from './ProfileCard';

/** Gerenciador de presets: importar, exportar, ver e excluir perfis. */
export function ProfilesScreen({ onImport }: { onImport: () => void }) {
  const [list, setList] = useState<ProfileList | null>(null);
  const [toDelete, setToDelete] = useState<ProfileSummary | null>(null);

  const reload = useCallback(async () => {
    const result = await commands.listProfiles();
    if (result.status === 'ok') setList(result.data);
    else toast.error(result.error.message);
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function exportProfile(profile: ProfileSummary) {
    const result = await commands.exportProfile(profile.id);
    if (result.status === 'error') toast.error(result.error.message);
    else if (result.data)
      toast.success(`Exportado: ${result.data}`, {
        description: 'O arquivo não tem credenciais. Mande a PSK do grupo por outro canal.',
      });
  }

  async function deleteProfile(forget: boolean) {
    if (!toDelete) return;
    const result = await commands.deleteProfile(toDelete.id, forget);
    setToDelete(null);
    if (result.status === 'ok') {
      toast.success('Perfil excluído');
      await reload();
    } else toast.error(result.error.message);
  }

  return (
    <div className="flex w-full flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">Perfis</h2>
        <Button onClick={onImport}>
          <FileUp />
          Importar
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Importe um perfil (.json) recebido da sua equipe ou o swanctl.conf de uma VPN que já
        funciona. Você também pode arrastar o arquivo para a janela.
      </p>

      {list?.profiles.length === 0 && (
        <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
          Nenhum perfil ainda.
        </p>
      )}

      {list?.profiles.map((p) => (
        <ProfileCard
          key={p.id}
          profile={p}
          onExport={() => void exportProfile(p)}
          onDelete={() => setToDelete(p)}
        />
      ))}

      {list && list.broken.length > 0 && (
        <div className="rounded-lg bg-warning/10 p-3 text-xs">
          <p className="flex items-center gap-1 font-medium">
            <TriangleAlert className="size-3.5 text-warning" aria-hidden />
            Arquivos ignorados
          </p>
          <ul className="mt-1 list-disc pl-5" data-selectable>
            {list.broken.map((b) => (
              <li key={b.file}>
                {b.file}: {b.error}
              </li>
            ))}
          </ul>
        </div>
      )}

      {toDelete && (
        <DeleteDialog
          profile={toDelete}
          onCancel={() => setToDelete(null)}
          onConfirm={(forget) => void deleteProfile(forget)}
        />
      )}
    </div>
  );
}
