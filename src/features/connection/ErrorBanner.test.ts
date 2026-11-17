import { describe, expect, it } from 'vitest';
import { splitMessage } from './ErrorBanner';

describe('splitMessage', () => {
  it('separates what happened from what to do', () => {
    expect(
      splitMessage(
        'The gateway rejected phase 1 authentication: check the group PSK and the group id',
      ),
    ).toEqual({
      what: 'The gateway rejected phase 1 authentication',
      next: 'Check the group PSK and the group id',
    });
  });

  it('keeps a message without a colon whole', () => {
    expect(splitMessage('Wrong XAuth username or password')).toEqual({
      what: 'Wrong XAuth username or password',
      next: null,
    });
    expect(splitMessage(': nothing before')).toEqual({ what: ': nothing before', next: null });
  });
});
