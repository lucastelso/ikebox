import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LogPanel } from './LogPanel';

const lines = ['established between 192.0.2.10[gr***te]...198.51.100.7', 'virtual IP 10.1.2.3'];

describe('LogPanel', () => {
  let writeText: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  });
  afterEach(() => {
    Reflect.deleteProperty(navigator, 'clipboard');
  });

  it('copia o texto anonimizado pelo Rust, nunca as linhas cruas', async () => {
    const anonymize = vi.fn().mockResolvedValue('established between <ip-1>...<gateway>');
    render(<LogPanel lines={lines} anonymize={anonymize} />);
    await userEvent.click(screen.getByText('Log da conexão'));
    await userEvent.click(screen.getByRole('button', { name: 'Copiar anonimizado' }));
    expect(anonymize).toHaveBeenCalledWith(lines);
    expect(writeText).toHaveBeenCalledWith('established between <ip-1>...<gateway>');
    expect(await screen.findByRole('button', { name: 'Copiado' })).toBeInTheDocument();
  });

  it('sem anonimização, não copia nada e avisa', async () => {
    const anonymize = vi.fn().mockResolvedValue(null);
    render(<LogPanel lines={lines} anonymize={anonymize} />);
    await userEvent.click(screen.getByText('Log da conexão'));
    await userEvent.click(screen.getByRole('button', { name: 'Copiar anonimizado' }));
    expect(writeText).not.toHaveBeenCalled();
    expect(await screen.findByText(/nada foi copiado/)).toBeInTheDocument();
  });
});
