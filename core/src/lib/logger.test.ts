import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { Logger } from './logger.js';

describe('Logger', () => {
  let logger: Logger;
  const componentName = 'TestComponent';

  beforeEach(() => {
    logger = new Logger(componentName);
    vi.spyOn(console, 'log').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.spyOn(console, 'debug').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('logs info messages with component prefix', () => {
    logger.info('test message', { data: 123 });
    expect(console.log).toHaveBeenCalledWith(`[${componentName}]`, 'test message', { data: 123 });
  });

  it('logs warn messages with component prefix', () => {
    logger.warn('test warning', 'issue');
    expect(console.warn).toHaveBeenCalledWith(`[${componentName}]`, 'test warning', 'issue');
  });

  it('logs error messages with component prefix', () => {
    logger.error('test error', new Error('fail'));
    expect(console.error).toHaveBeenCalledWith(
      `[${componentName}]`,
      'test error',
      expect.any(Error)
    );
  });

  it('debug method does not throw and does not log (commented out in implementation)', () => {
    logger.debug('debug message');
    expect(console.debug).not.toHaveBeenCalled();
  });
});
