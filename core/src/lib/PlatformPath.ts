export class PlatformPath {
  /**
   * Normalizes a Windows path to be compatible with Docker bind mounts.
   * E.g., C:/path/to/dir -> /c/path/to/dir
   */
  static normalizeWindowsPath(p: string): string {
    if (process.platform === 'win32' && /^[a-zA-Z]:/.test(p)) {
      return `/${p[0]!.toLowerCase()}${p.slice(2).replace(/\\/g, '/')}`;
    }
    return p;
  }
}
