import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration } from './format';

describe('formatting', () => {
  it('formats bytes with the decimal separator of the language', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1536)).toBe('1.5 KiB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MiB');
    expect(formatBytes(1536, 'pt')).toBe('1,5 KiB');
  });

  it('formats durations', () => {
    expect(formatDuration(5)).toBe('5 s');
    expect(formatDuration(125)).toBe('2 min 5 s');
    expect(formatDuration(3720)).toBe('1 h 2 min');
  });
});
