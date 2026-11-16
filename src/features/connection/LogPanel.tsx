import { Check, Copy, ScrollText } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';

/**
 * Log do charon (identidades já mascaradas pelo Rust), recolhível e copiável.
 * O texto copiado passa por `anonymize` (Rust): gateway, IPs e domínios viram
 * marcadores, para o log poder ir num relatório de bug.
 */
export function LogPanel({
  lines,
  anonymize,
}: {
  lines: string[];
  /** Devolve o texto anonimizado, ou `null` se não deu (nada é copiado). */
  anonymize: (lines: string[]) => Promise<string | null>;
}) {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const endRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (lines.length > 0) endRef.current?.scrollIntoView?.({ block: 'end' });
  }, [lines]);

  async function copy() {
    setCopyFailed(false);
    const text = await anonymize(lines);
    if (text === null) {
      // Sem anonimizar, nada vai para a área de transferência.
      setCopyFailed(true);
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
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
        <div className="flex items-center justify-between gap-2 px-4 pt-2">
          <p className="text-xs text-muted-foreground">
            {copyFailed
              ? 'Não foi possível anonimizar o log; nada foi copiado.'
              : 'Ao copiar, gateway, IPs e domínios viram marcadores. Confira antes de compartilhar.'}
          </p>
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            onClick={() => void copy()}
            disabled={lines.length === 0}
          >
            {copied ? <Check /> : <Copy />}
            {copied ? 'Copiado' : 'Copiar anonimizado'}
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
