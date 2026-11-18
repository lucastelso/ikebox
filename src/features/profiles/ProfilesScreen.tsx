import { FilePlus, FileUp, TriangleAlert } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import {
  commands,
  type ProfileDraft,
  type ProfileList,
  type ProfileSummary,
  type ProfileTemplate,
} from '@/bindings';
import { Button } from '@/components/ui/button';
import { useT } from '@/i18n';
import { DeleteDialog } from './DeleteDialog';
import { ProfileCard } from './ProfileCard';
import { ProfileEditor } from './ProfileEditor';
import { TemplateDialog } from './TemplateDialog';

/** Profile manager: create, edit, import, export, view and delete. */
export function ProfilesScreen({ onImport }: { onImport: () => void }) {
  const t = useT();
  const [list, setList] = useState<ProfileList | null>(null);
  const [toDelete, setToDelete] = useState<ProfileSummary | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [editing, setEditing] = useState<ProfileDraft | null>(null);

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
      toast.success(t.profiles.exported(result.data), {
        description: t.profiles.exportedHint,
      });
  }

  async function startNew(template: ProfileTemplate) {
    setChoosing(false);
    setEditing(await commands.newProfileDraft(template));
  }

  async function startEdit(profile: ProfileSummary) {
    const result = await commands.editProfileDraft(profile.id);
    if (result.status === 'ok') setEditing(result.data);
    else toast.error(result.error.message);
  }

  async function deleteProfile(forget: boolean) {
    if (!toDelete) return;
    const result = await commands.deleteProfile(toDelete.id, forget);
    setToDelete(null);
    if (result.status === 'ok') {
      toast.success(t.profiles.deleted);
      await reload();
    } else toast.error(result.error.message);
  }

  if (editing) {
    return (
      <ProfileEditor
        initial={editing}
        onCancel={() => setEditing(null)}
        onSaved={() => {
          setEditing(null);
          void reload();
        }}
      />
    );
  }

  return (
    <div className="flex w-full flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-semibold">{t.profiles.title}</h2>
        <div className="flex gap-2">
          <Button variant="secondary" onClick={() => setChoosing(true)}>
            <FilePlus />
            {t.editor.newProfile}
          </Button>
          <Button onClick={onImport}>
            <FileUp />
            {t.profiles.import}
          </Button>
        </div>
      </div>
      <p className="text-xs text-muted-foreground">{t.profiles.intro}</p>

      {list?.profiles.length === 0 && (
        <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
          {t.profiles.none}
        </p>
      )}

      {list?.profiles.map((p) => (
        <ProfileCard
          key={p.id}
          profile={p}
          onEdit={() => void startEdit(p)}
          onExport={() => void exportProfile(p)}
          onDelete={() => setToDelete(p)}
        />
      ))}

      {list && list.broken.length > 0 && (
        <div className="rounded-lg bg-warning/10 p-3 text-xs">
          <p className="flex items-center gap-1 font-medium">
            <TriangleAlert className="size-3.5 text-warning" aria-hidden />
            {t.profiles.ignored}
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

      {choosing && (
        <TemplateDialog
          onChoose={(template) => void startNew(template)}
          onCancel={() => setChoosing(false)}
        />
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
