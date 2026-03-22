export class PlatformPath {
  /**
   * Normalizes a host path for Docker bind mounts on Windows.
   * Converts "C:\path" to "/c/path" format.
   * On non-Windows platforms or if the path is not absolute, returns the original path.
   */
  static normalizeDockerBindPath(hostPath: string): string {
    if (process.platform === 'win32' && /^[a-zA-Z]:/.test(hostPath)) {
      return `/${hostPath[0]!.toLowerCase()}${hostPath.slice(2).replace(/\\/g, '/')}`;
    }
    return hostPath;
  }
}
