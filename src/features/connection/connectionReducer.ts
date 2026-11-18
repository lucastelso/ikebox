import type { Connection, SessionUpdate, UiError } from '@/bindings';

/** Connection phase as the UI sees it. */
export type Phase =
  | 'idle'
  | 'preparing'
  | 'negotiating'
  | 'connected'
  | 'reconnecting'
  | 'disconnecting';

export interface ConnectionState {
  phase: Phase;
  /** Current preparation step (e.g. "engine image"). */
  step: string | null;
  connection: Connection | null;
  error: UiError | null;
  /** Last log lines (identities already masked by Rust). */
  logs: string[];
  /** The engine image is being built (first use). */
  building: boolean;
}

export const MAX_LOG_LINES = 300;

export const initialState: ConnectionState = {
  phase: 'idle',
  step: null,
  connection: null,
  error: null,
  logs: [],
  building: false,
};

export type Action =
  | { type: 'connectStarted' }
  | { type: 'connectSucceeded'; connection: Connection }
  | { type: 'connectFailed'; error: UiError }
  | { type: 'disconnectStarted' }
  | { type: 'disconnected' }
  | { type: 'disconnectFailed'; error: UiError }
  | { type: 'statusPolled'; connection: Connection | null }
  | { type: 'sessionUpdate'; update: SessionUpdate }
  | { type: 'dismissError' };

/** Phases with an operation in progress (the status reading does not interfere). */
export function isBusy(phase: Phase): boolean {
  return phase === 'preparing' || phase === 'negotiating' || phase === 'disconnecting';
}

function appendLog(logs: string[], line: string): string[] {
  const next = logs.length >= MAX_LOG_LINES ? logs.slice(logs.length - MAX_LOG_LINES + 1) : logs;
  return [...next, line];
}

export function connectionReducer(state: ConnectionState, action: Action): ConnectionState {
  switch (action.type) {
    case 'connectStarted':
      return { ...initialState, phase: 'preparing', step: 'iniciando' };
    case 'connectSucceeded':
      return {
        ...state,
        phase: action.connection.established ? 'connected' : 'reconnecting',
        step: null,
        connection: action.connection,
        building: false,
      };
    case 'connectFailed':
      return { ...state, phase: 'idle', step: null, error: action.error, building: false };
    case 'disconnectStarted':
      return { ...state, phase: 'disconnecting', error: null };
    case 'disconnected':
      return { ...state, phase: 'idle', connection: null, step: null };
    case 'disconnectFailed':
      return { ...state, phase: state.connection ? 'connected' : 'idle', error: action.error };
    case 'statusPolled': {
      if (isBusy(state.phase)) return state;
      if (!action.connection) {
        return state.phase === 'idle' ? state : { ...state, phase: 'idle', connection: null };
      }
      return {
        ...state,
        phase: action.connection.established ? 'connected' : 'reconnecting',
        connection: action.connection,
      };
    }
    case 'sessionUpdate':
      return applyUpdate(state, action.update);
    case 'dismissError':
      return { ...state, error: null };
  }
}

function applyUpdate(state: ConnectionState, update: SessionUpdate): ConnectionState {
  switch (update.kind) {
    case 'preparing':
      return { ...state, phase: 'preparing', step: update.step };
    case 'negotiating':
      return { ...state, phase: 'negotiating', step: null };
    case 'connected':
      return {
        ...state,
        phase: 'connected',
        step: null,
        connection: update.connection,
        building: false,
      };
    case 'log':
      return { ...state, logs: appendLog(state.logs, update.line) };
    case 'build':
      return { ...state, building: true, logs: appendLog(state.logs, `[build] ${update.line}`) };
    case 'tunnelDown':
      return state.phase === 'connected' ? { ...state, phase: 'reconnecting' } : state;
    case 'tunnelUp':
      return state.phase === 'reconnecting' ? { ...state, phase: 'connected' } : state;
    case 'engineGone':
      return isBusy(state.phase) ? state : { ...state, phase: 'idle', connection: null };
  }
}
