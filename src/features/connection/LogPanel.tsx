import { Check, Copy, ScrollText } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';

/** Log do charon (identidades já mascaradas pelo Rust), recolhível e copiável. */
export function LogPanel({ lines }: { lines: string[] }) {
  const [copied, setCopied] = useState(false);
  const endRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (lines.length > 0) endRef.current?.scrollIntoView?.({ block: 'end' });
  }, [lines]);

  async function copy() {
    try {
      await navigator.clipboard.writeText(lines.join('\n'));
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Sem área de transferência: o usuário ainda pode selecionar o texto.
    }
  }

  return (
    <details className="group w-full rounded-xl border bg-card">
      <summary className="flex cursor-pointer select-none items-center gap-2 px-4 py-3 text-sm font-medium">
        <ScrollText className="size-4 text-muted-foreground" aria-hidden />
        Log da conexão
        <span className="text-xs font-normal text-muted-foreground">({lines.length} linhas)</span>
      </summary>
      <div className="border-t">
        <div className="flex justify-end px-2 pt-2">
          <Button size="sm" variant="ghost" onClick={copy} disabled={lines.length === 0}>
            {copied ? <Check /> : <Copy />}
            {copied ? 'Copiado' : 'Copiar'}
          </Button>
        </div>
        <pre
          className="max-h-56 overflow-auto whitespace-pre-wrap break-words px-4 pb-4 font-mono text-xs leading-relaxed text-muted-foreground"
          data-selectable
        >
          {lines.length === 0 ? 'Sem registros ainda.' : lines.join('\n')}
          <span ref={endRef} />
        </pre>
      </div>
    </details>
  );
}
