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

  it('copies the text anonymized by Rust, never the raw lines', async () => {
    const anonymize = vi.fn().mockResolvedValue('established between <ip-1>...<gateway>');
    render(<LogPanel lines={lines} anonymize={anonymize} />);
    await userEvent.click(screen.getByText('Connection log'));
    await userEvent.click(screen.getByRole('button', { name: 'Copy anonymized' }));
    expect(anonymize).toHaveBeenCalledWith(lines);
    expect(writeText).toHaveBeenCalledWith('established between <ip-1>...<gateway>');
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument();
  });

  it('without anonymization, copies nothing and warns', async () => {
    const anonymize = vi.fn().mockResolvedValue(null);
    render(<LogPanel lines={lines} anonymize={anonymize} />);
    await userEvent.click(screen.getByText('Connection log'));
    await userEvent.click(screen.getByRole('button', { name: 'Copy anonymized' }));
    expect(writeText).not.toHaveBeenCalled();
    expect(await screen.findByText(/nothing was copied/)).toBeInTheDocument();
  });
});
