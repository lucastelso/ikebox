import { mockIPC } from '@tauri-apps/api/mocks';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';

describe('App (scaffold)', () => {
  it('mostra a versão vinda do Rust pelo IPC', async () => {
    mockIPC((cmd) => {
      if (cmd === 'app_info') return { version: '9.9.9', engineImage: 'engine:teste' };
      return undefined;
    });
    render(<App />);
    expect(await screen.findByText('9.9.9')).toBeInTheDocument();
    expect(screen.getByText('engine:teste')).toBeInTheDocument();
  });

  it('mostra o erro se o IPC falhar', async () => {
    mockIPC(() => {
      throw new Error('falhou');
    });
    render(<App />);
    expect(await screen.findByText(/falhou/)).toBeInTheDocument();
  });
});
