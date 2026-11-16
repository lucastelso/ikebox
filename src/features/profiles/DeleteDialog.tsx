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

/** Confirma a exclusão de um perfil (e, opcionalmente, das credenciais). */
export function DeleteDialog({
  profile,
  onCancel,
  onConfirm,
}: {
  profile: ProfileSummary;
  onCancel: () => void;
  onConfirm: (forgetCredentials: boolean) => void;
}) {
  const id = useId();
  const [forget, setForget] = useState(true);
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Excluir perfil?</DialogTitle>
          <DialogDescription>
            "{profile.name}" será removido deste computador. Você pode importá-lo de novo depois.
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2">
          <Checkbox id={id} checked={forget} onCheckedChange={(c) => setForget(c === true)} />
          <Label htmlFor={id}>Apagar também as credenciais guardadas</Label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onCancel}>
            Cancelar
          </Button>
          <Button variant="destructive" onClick={() => onConfirm(forget)}>
            Excluir
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
