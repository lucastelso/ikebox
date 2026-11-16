import { describe, expect, it } from 'vitest';
import type { Connection, UiError } from '@/bindings';
import {
  type ConnectionState,
  connectionReducer,
  initialState,
  MAX_LOG_LINES,
} from './connectionReducer';

const conn: Connection = {
  profileId: 'vpn',
  established: true,
  gateway: '203.0.113.1',
  vip: '198.51.100.10',
  remoteTs: ['192.0.2.0/24'],
  establishedSecs: 10,
  bytesIn: 100,
  bytesOut: 50,
};

const error: UiError = {
  kind: 'negotiation',
  message: 'Usuário ou senha XAuth incorretos',
  details: [],
};

function run(...actions: Parameters<typeof connectionReducer>[1][]): ConnectionState {
  return actions.reduce(connectionReducer, initialState);
}

describe('connectionReducer', () => {
  it('walks the connection flow through the events', () => {
    const s = run(
      { type: 'connectStarted' },
      { type: 'sessionUpdate', update: { kind: 'preparing', step: 'subindo o engine' } },
      { type: 'sessionUpdate', update: { kind: 'negotiating' } },
      { type: 'sessionUpdate', update: { kind: 'log', line: 'IKE_SA established' } },
      { type: 'connectSucceeded', connection: conn },
    );
    expect(s.phase).toBe('connected');
    expect(s.connection).toEqual(conn);
    expect(s.logs).toEqual(['IKE_SA established']);
  });

  it('keeps the error and goes back to disconnected', () => {
    const s = run({ type: 'connectStarted' }, { type: 'connectFailed', error });
    expect(s.phase).toBe('idle');
    expect(s.error).toEqual(error);
    expect(run({ type: 'connectFailed', error }, { type: 'dismissError' }).error).toBeNull();
  });

  it('reconnects when the tunnel drops and comes back', () => {
    const connected = run({ type: 'connectSucceeded', connection: conn });
    const down = connectionReducer(connected, {
      type: 'sessionUpdate',
      update: { kind: 'tunnelDown' },
    });
    expect(down.phase).toBe('reconnecting');
    expect(
      connectionReducer(down, { type: 'sessionUpdate', update: { kind: 'tunnelUp' } }).phase,
    ).toBe('connected');
  });

  it('periodic status adopts connections opened outside the app without disturbing operations', () => {
    expect(run({ type: 'statusPolled', connection: conn }).phase).toBe('connected');
    const busy = run({ type: 'connectStarted' }, { type: 'statusPolled', connection: null });
    expect(busy.phase).toBe('preparing');
    const gone = run(
      { type: 'connectSucceeded', connection: conn },
      { type: 'statusPolled', connection: null },
    );
    expect(gone.phase).toBe('idle');
    const noTunnel = run({ type: 'statusPolled', connection: { ...conn, established: false } });
    expect(noTunnel.phase).toBe('reconnecting');
  });

  it('disconnection and stopped engine', () => {
    const s = run(
      { type: 'connectSucceeded', connection: conn },
      { type: 'disconnectStarted' },
      { type: 'disconnected' },
    );
    expect(s.phase).toBe('idle');
    expect(s.connection).toBeNull();
    const gone = run(
      { type: 'connectSucceeded', connection: conn },
      { type: 'sessionUpdate', update: { kind: 'engineGone' } },
    );
    expect(gone.phase).toBe('idle');
  });

  it('caps the log and flags the image build', () => {
    let s = initialState;
    for (let i = 0; i < MAX_LOG_LINES + 5; i++) {
      s = connectionReducer(s, { type: 'sessionUpdate', update: { kind: 'log', line: `l${i}` } });
    }
    expect(s.logs).toHaveLength(MAX_LOG_LINES);
    expect(s.logs.at(-1)).toBe(`l${MAX_LOG_LINES + 4}`);
    const b = connectionReducer(s, {
      type: 'sessionUpdate',
      update: { kind: 'build', line: 'Step 1' },
    });
    expect(b.building).toBe(true);
    expect(b.logs.at(-1)).toBe('[build] Step 1');
  });
});
