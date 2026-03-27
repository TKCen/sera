import { describe, it, expect, vi, afterEach } from 'vitest';
import { PlatformPath } from './PlatformPath';

describe('PlatformPath', () => {
  const originalPlatform = process.platform;

  afterEach(() => {
    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
    });
  });

  describe('normalizeDockerBindPath', () => {
    it('should normalize Windows absolute path correctly', () => {
      Object.defineProperty(process, 'platform', {
        value: 'win32',
      });
      const input = 'C:\\path\\to\\dir';
      expect(PlatformPath.normalizeDockerBindPath(input)).toBe('/c/path/to/dir');
    });

    it('should normalize lowercase Windows absolute path correctly', () => {
      Object.defineProperty(process, 'platform', {
        value: 'win32',
      });
      const input = 'd:\\Another\\Path';
      expect(PlatformPath.normalizeDockerBindPath(input)).toBe('/d/Another/Path');
    });

    it('should not modify non-absolute path on Windows', () => {
      Object.defineProperty(process, 'platform', {
        value: 'win32',
      });
      const input = 'path\\to\\dir';
      expect(PlatformPath.normalizeDockerBindPath(input)).toBe(input);
    });

    it('should not modify Unix path on Windows', () => {
      Object.defineProperty(process, 'platform', {
        value: 'win32',
      });
      const input = '/path/to/dir';
      expect(PlatformPath.normalizeDockerBindPath(input)).toBe(input);
    });

    it('should not modify path on non-Windows platforms', () => {
      Object.defineProperty(process, 'platform', {
        value: 'linux',
      });
      const input = 'C:\\path\\to\\dir';
      expect(PlatformPath.normalizeDockerBindPath(input)).toBe(input);
    });
  });
});
