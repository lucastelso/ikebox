import { useId, useState } from 'react';
import type { ProfileSummary } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Label } from '@/components/ui/label';
import { useT } from '@/i18n';

/** Confirms deleting a profile (and, optionally, its credentials). */
export function DeleteDialog({
  profile,
  onCancel,
  onConfirm,
}: {
  profile: ProfileSummary;
  onCancel: () => void;
  onConfirm: (forgetCredentials: boolean) => void;
}) {
  const t = useT().profiles;
  const id = useId();
  const [forget, setForget] = useState(true);
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t.deleteTitle}</DialogTitle>
          <DialogDescription>{t.deleteBody(profile.name)}</DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <Checkbox id={id} checked={forget} onCheckedChange={(c) => setForget(c === true)} />
          <Label htmlFor={id}>{t.deleteCredentials}</Label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onCancel}>
            {t.cancel}
          </Button>
          <Button variant="destructive" onClick={() => onConfirm(forget)}>
            {t.confirmDelete}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
