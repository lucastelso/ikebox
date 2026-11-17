import { ChevronRight } from 'lucide-react';
import type { ProfileTemplate } from '@/bindings';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useT } from '@/i18n';

const TEMPLATES: ProfileTemplate[] = [
  'fortigate',
  'ciscoEzvpn',
  'ikev2Psk',
  'ikev2Eap',
  'ikev2Cert',
  'blank',
];

/** Where a new profile starts from: a vendor template or blank. */
export function TemplateDialog({
  onChoose,
  onCancel,
}: {
  onChoose: (template: ProfileTemplate) => void;
  onCancel: () => void;
}) {
  const t = useT().editor;
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t.chooseTemplate}</DialogTitle>
          <DialogDescription>{t.chooseTemplateHint}</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2">
          {TEMPLATES.map((template) => (
            <button
              key={template}
              type="button"
              onClick={() => onChoose(template)}
              className="flex items-center gap-3 rounded-lg border bg-card p-3 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
            >
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-medium">{t.template[template]}</span>
                <span className="block text-xs text-muted-foreground">
                  {t.templateHint[template]}
                </span>
              </span>
              <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden />
            </button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
