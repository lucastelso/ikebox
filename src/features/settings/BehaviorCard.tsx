import { useId } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { useT } from '@/i18n';
import { useAppSettings } from './useAppSettings';

function Option({
  id,
  label,
  hint,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  hint: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-2">
      <Checkbox id={id} checked={checked} onCheckedChange={(c) => onChange(c === true)} />
      <div className="grid gap-0.5">
        <Label htmlFor={id}>{label}</Label>
        <p className="text-xs text-muted-foreground">{hint}</p>
      </div>
    </div>
  );
}

/** What closing the window does, and the optional update notice. */
export function BehaviorCard() {
  const t = useT().system;
  const id = useId();
  const { settings, setCloseToTray, setUpdateCheck } = useAppSettings();
  if (!settings) return null;
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t.behavior}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Option
          id={`${id}-tray`}
          label={t.closeToTray}
          hint={t.closeToTrayHint}
          checked={settings.closeToTray}
          onChange={(v) => void setCloseToTray(v)}
        />
        {settings.updateCheckAvailable && (
          <Option
            id={`${id}-update`}
            label={t.updateCheck}
            hint={t.updateCheckHint}
            checked={settings.updateCheck}
            onChange={(v) => void setUpdateCheck(v)}
          />
        )}
      </CardContent>
    </Card>
  );
}
