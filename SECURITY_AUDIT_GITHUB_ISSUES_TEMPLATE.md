# GitHub Issues Template — Security Audit Fixes

Use these templates to file security issues in GitHub. Each issue corresponds to a finding in the security audit report.

---

## CRITICAL-001: Code Execution Sandbox Escape

**Title:** CRITICAL: Code execution sandbox can be escaped via prototype chain manipulation

**Labels:** `security`, `critical`, `code-execution`, `sandbox`

**Assignee:** [Development team lead]

**Deadline:** Immediate (block production)

**Description:**

The code evaluation sandbox in `core/agent-runtime/src/tools/code-handler.ts` (line 62) is fundamentally insecure and can be escaped via prototype chain manipulation.

**Current Implementation (Vulnerable):**

```typescript
const fn = new Function('sandbox', `with(sandbox) { ${code} }`);
fn(sandbox);
```

**Problem:**

- The `with(sandbox)` statement does NOT create an isolated context
- Attackers can access the global object via `(function(){return this})()` or `globalThis`
- Unrestricted code execution with full process context access
- Can access `process.env`, spawn child processes, breakout to host

**Security Impact:**

- Severity: CRITICAL
- Exploitability: Remote, by agents with code-execution capability
- Blast Radius: RCE, host system breakout

**Remediation:**
Replace with `isolated-vm` for proper sandboxing:

```typescript
import ivm from 'isolated-vm';

export async function evaluateCode(code: string, timeout: number = 5000): Promise<string> {
  const isolate = new ivm.Isolate({ memoryLimit: 128 });
  const context = isolate.createContextSync();

  try {
    const script = isolate.compileScriptSync(code);
    const result = await script.run(context, { timeout });
    return JSON.stringify(result);
  } catch (err) {
    return `Error: Code execution failed: ${err instanceof Error ? err.message : String(err)}`;
  } finally {
    isolate.dispose();
  }
}
```

**Testing:**

- [ ] Code evaluation still works for safe code
- [ ] Prototype chain manipulation is blocked
- [ ] Process access is blocked
- [ ] Child process spawning is blocked

---

## CRITICAL-002: Incomplete SSRF Filtering

**Title:** CRITICAL: SSRF filtering allows access to AWS metadata service and reserved IP ranges

**Labels:** `security`, `critical`, `ssrf`, `network`

**Assignee:** [Development team lead]

**Deadline:** Immediate (block production)

**Description:**

The SSRF filtering in `http-handler.ts` (lines 19-22) and `web-handler.ts` (lines 25-28) is incomplete and allows:

- `0.0.0.0` (current host, all interfaces)
- `169.254.x.x` (AWS EC2 metadata service)
- `224.0.0.x` - `239.255.255.255` (multicast)
- `240.0.0.0` - `255.255.255.255` (reserved/broadcast)

**Current Implementation (Vulnerable):**

```typescript
const SERA_INTERNAL_HOSTS = /^https?:\/\/(sera-core|sera-db|...)/i;
if (
  !SERA_INTERNAL_HOSTS.test(url) &&
  /^https?:\/\/(localhost|127\.|10\.|172\.(1[6-9]|2\d|3[01])\.|192\.168\.)/i.test(url)
) {
  return 'Error: ...';
}
// Allows: http://0.0.0.0:80, http://169.254.169.254 (AWS metadata)
```

**Security Impact:**

- Severity: CRITICAL
- Exploitability: Remote, by agents with http/web-fetch capability
- Blast Radius: Access to internal services, metadata exfiltration, cloud credential theft

**Remediation:**
Use explicit allow-list and comprehensive private IP blocking:

```typescript
function isSSRFBlocked(url: string): boolean {
  try {
    const parsed = new URL(url);
    const hostname = parsed.hostname;

    // Allow-list: only SERA internal services
    const ALLOWED_HOSTS = new Set([
      'sera-core',
      'sera-db',
      'sera-centrifugo',
      'centrifugo',
      'sera-qdrant',
      'sera-egress-proxy',
    ]);
    if (ALLOWED_HOSTS.has(hostname.toLowerCase())) return false;

    // Block ALL private and reserved IP ranges
    const IP_PATTERN = /^(\d+)\.(\d+)\.(\d+)\.(\d+)$/;
    const match = hostname.match(IP_PATTERN);
    if (!match) return false;

    const [, a, b, c, d] = match.map(Number);

    // RFC 1918 private ranges
    if (a === 10) return true;
    if (a === 172 && b >= 16 && b <= 31) return true;
    if (a === 192 && b === 168) return true;

    // Loopback
    if (a === 127) return true;

    // Link-local (169.254.x.x) — AWS metadata
    if (a === 169 && b === 254) return true;

    // This host (0.0.0.0)
    if (a === 0) return true;

    // Multicast (224.0.0.0 - 239.255.255.255)
    if (a >= 224 && a <= 239) return true;

    // Reserved (240.0.0.0 - 255.255.255.255)
    if (a >= 240) return true;

    return false;
  } catch {
    return true;
  }
}
```

**Testing:**

- [ ] Public IPs are allowed
- [ ] All private ranges are blocked (10.x, 172.16-31.x, 192.168.x.x)
- [ ] AWS metadata (169.254.169.254) is blocked
- [ ] Multicast (224+) is blocked
- [ ] Reserved (240+) is blocked
- [ ] SERA internal services still work (sera-core, etc.)

---

## CRITICAL-003: Symlink Path Traversal

**Title:** CRITICAL: Path validation bypassed by symlinks and hardlinks

**Labels:** `security`, `critical`, `path-traversal`, `file-access`

**Assignee:** [Development team lead]

**Deadline:** Immediate (block production)

**Description:**

The `resolveSafe()` function in `file-handlers.ts` (lines 143-151) uses `startsWith()` comparison which does NOT account for symlinks. Attackers can escape workspace by creating symlinks inside the workspace pointing outside.

**Current Implementation (Vulnerable):**

```typescript
export function resolveSafe(workspacePath: string, filePath: string): string {
  const resolved = path.resolve(workspacePath, filePath);
  if (!resolved.startsWith(workspacePath)) {
    throw new Error('Path traversal detected');
  }
  return resolved;
}
// Can be bypassed if workspace or target contains symlinks
```

**Security Impact:**

- Severity: CRITICAL
- Exploitability: Remote, by agents with file-read/write capability
- Blast Radius: Arbitrary file read/write outside workspace

**Remediation:**
Use `fs.realpathSync()` to resolve symlinks before validation:

```typescript
export function resolveSafe(workspacePath: string, filePath: string): string {
  try {
    const realWorkspace = fs.realpathSync(workspacePath);
    const requestedPath = path.resolve(workspacePath, filePath);
    const realRequestedPath = fs.realpathSync(requestedPath);

    // Check real paths, not just string prefix
    if (
      !realRequestedPath.startsWith(realWorkspace + path.sep) &&
      realRequestedPath !== realWorkspace
    ) {
      throw new Error('Path traversal detected');
    }

    return realRequestedPath;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
      throw new Error(`Path does not exist: ${filePath}`);
    }
    throw err;
  }
}
```

**Testing:**

- [ ] Legitimate file reads/writes still work
- [ ] Symlinks inside workspace are followed
- [ ] Symlinks pointing outside workspace are blocked
- [ ] Hardlinks outside workspace are blocked

---

## CRITICAL-004: Shell Path Validation Regex Bypass

**Title:** CRITICAL: Shell path restriction regex can be bypassed with relative paths

**Labels:** `security`, `critical`, `shell-injection`, `command-injection`

**Assignee:** [Development team lead]

**Deadline:** Immediate (block production)

**Description:**

The shell path restriction in `shell-handler.ts` (line 213) uses an incomplete regex that can be bypassed with relative paths, quoted spaces, and file:// URLs.

**Current Implementation (Vulnerable):**

```typescript
const checkShellPathRestriction = (cmd: string) => {
  const restrictedPaths = cmd.match(/(?:^|\s)(\/(?!workspace\b)[^\s]+)/g);
  if (restrictedPaths) {
    return `Error: Shell commands cannot reference paths outside the workspace`;
  }
  return null;
};

// These bypass the check:
// "cd ../../etc && cat passwd"
// "cd ./workspace/../../../etc"
// "cat '/etc/pass word'"
// "curl file:///etc/passwd"
```

**Security Impact:**

- Severity: CRITICAL
- Exploitability: Remote, by tier-2+ agents with shell-exec capability
- Blast Radius: Arbitrary shell command execution inside container

**Remediation:**
Use blocklist of dangerous patterns instead of regex:

```typescript
function checkShellPathRestriction(cmd: string): string | null {
  const blockedPatterns = [
    /\.\.\//, // relative traversal
    /~\/|~$/, // home directory
    /\$\{?HOME\}?/i, // $HOME variable
    /\/etc\//i, // /etc access
    /\/sys\//i, // /sys access
    /\/proc\//i, // /proc access
    /\/dev\//i, // /dev access
    /file:\/\//i, // file:// URLs
  ];

  for (const pattern of blockedPatterns) {
    if (pattern.test(cmd)) {
      return `Error: Shell command contains blocked pattern`;
    }
  }

  // Block dangerous commands
  const tokens = cmd.split(/\s+/);
  const blockedCommands = new Set([
    'sudo',
    'su',
    'mount',
    'umount',
    'chroot',
    'docker',
    'podman',
    'lxc',
    'curl',
    'wget',
    'nc',
    'ncat',
  ]);

  if (tokens.length > 0 && blockedCommands.has(tokens[0])) {
    return `Error: Command '${tokens[0]}' is not allowed`;
  }

  return null;
}
```

**Testing:**

- [ ] Legitimate workspace commands work
- [ ] Relative traversal (../) is blocked
- [ ] Home directory expansion is blocked
- [ ] /etc, /sys, /proc access is blocked
- [ ] Dangerous commands (sudo, curl, docker) are blocked

---

## HIGH-001: Ripgrep Pattern Injection

**Title:** HIGH: Agents can inject ripgrep flags via pattern parameter

**Labels:** `security`, `high`, `command-injection`, `glob`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

The `globFiles()` and `grepFiles()` functions in `search-handlers.ts` pass the `pattern` parameter directly to ripgrep without escaping. Agents can inject flags to bypass restrictions.

**Files:**

- `search-handlers.ts:18` (globFiles)
- `search-handlers.ts:58` (grepFiles — files_with_matches mode)
- `search-handlers.ts:80` (grepFiles — count mode)
- `search-handlers.ts:112` (grepFiles — content mode)

**Remediation:**
Use Node.js glob library instead of spawning ripgrep:

```typescript
import { glob } from 'glob';

export async function globFiles(workspacePath: string, pattern: string): Promise<string> {
  // Validate pattern — reject if contains flags or special chars
  if (pattern.startsWith('-') || /[;&|`$]/.test(pattern)) {
    return JSON.stringify({ error: 'Invalid pattern' });
  }

  try {
    const files = await glob(pattern, {
      cwd: workspacePath,
      ignore: ['node_modules/**', '.git/**'],
      maxDepth: 50,
    });
    return JSON.stringify({
      files: files.slice(0, 1000),
      total: files.length,
      truncated: files.length > 1000,
    });
  } catch (err) {
    return JSON.stringify({
      error: `Glob error: ${err instanceof Error ? err.message : String(err)}`,
    });
  }
}
```

---

## HIGH-002: Silent Error Swallowing in Poll Loop

**Title:** HIGH: Task polling silently retries indefinitely without backoff

**Labels:** `security`, `high`, `authentication`, `reliability`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

The task polling loop in `index.ts` (line 266) catches errors but continues indefinitely without backoff. If `SERA_IDENTITY_TOKEN` is invalid or missing, requests fail silently and auth failures are never detected.

**Remediation:**

Implement exponential backoff with max retry limit:

```typescript
let consecutiveErrors = 0;
const MAX_CONSECUTIVE_ERRORS = 3;
const INITIAL_BACKOFF_MS = 1000;

while (true) {
  try {
    const taskJson = await readTaskFromStdin(30000);
    // Process task...
    consecutiveErrors = 0;
  } catch (err) {
    consecutiveErrors++;
    const backoffMs = INITIAL_BACKOFF_MS * Math.pow(2, Math.min(consecutiveErrors - 1, 5));

    log(
      'error',
      `Failed to read task (attempt ${consecutiveErrors}): ${err instanceof Error ? err.message : String(err)}`
    );

    if (consecutiveErrors >= MAX_CONSECUTIVE_ERRORS) {
      log('error', `FATAL: Max consecutive errors reached. Exiting.`);
      process.exit(1);
    }

    await new Promise((resolve) => setTimeout(resolve, backoffMs));
  }
}
```

---

## HIGH-003: Semaphore Deadlock — No Timeout on Acquire

**Title:** HIGH: Semaphore.acquire() has no timeout; can deadlock indefinitely

**Labels:** `security`, `high`, `deadlock`, `concurrency`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

The Semaphore implementation in `executor.ts` (lines 84-107) has no timeout on `acquire()`. If a write-tool fails to call `release()`, subsequent tools deadlock indefinitely.

**Remediation:**

Add timeout and guarantee release via try-finally:

```typescript
export class Semaphore {
  private queue: (() => void)[] = [];
  private permits: number;
  private readonly timeout: number;

  constructor(permits: number = 1, timeout: number = 30000) {
    this.permits = permits;
    this.timeout = timeout;
  }

  async acquire() {
    if (this.permits > 0) {
      this.permits--;
      return;
    }

    return new Promise<void>((resolve, reject) => {
      const timeoutHandle = setTimeout(() => {
        const index = this.queue.indexOf(resolve);
        if (index !== -1) {
          this.queue.splice(index, 1);
        }
        reject(new Error('Semaphore acquire timeout'));
      }, this.timeout);

      this.queue.push(() => {
        clearTimeout(timeoutHandle);
        resolve();
      });
    });
  }

  release() {
    const next = this.queue.shift();
    if (next) next();
    else this.permits++;
  }
}

// Usage:
async function executeWithSemaphore(tool: Tool) {
  await semaphore.acquire();
  try {
    return await executeTool(tool);
  } finally {
    semaphore.release();
  }
}
```

---

## HIGH-004: Missing Token Validation in Proxy

**Title:** HIGH: Proxy requests sent with "Bearer undefined" if token missing

**Labels:** `security`, `high`, `authentication`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

The tool proxy in `proxy.ts` (lines 64-65) uses `SERA_IDENTITY_TOKEN` without validation. If missing, requests are sent with `Authorization: Bearer undefined`.

**Remediation:**

Validate token at startup and in each request:

```typescript
// At index.ts startup:
const SERA_IDENTITY_TOKEN = process.env.SERA_IDENTITY_TOKEN;
if (!SERA_IDENTITY_TOKEN || SERA_IDENTITY_TOKEN.trim() === '') {
  log('error', 'FATAL: SERA_IDENTITY_TOKEN is not set or empty');
  process.exit(1);
}

// In proxy.ts:
if (!token || token.trim() === '') {
  return JSON.stringify({
    error: 'Authentication token is missing or empty',
    tool: toolName,
  });
}
```

---

## HIGH-005: Unbounded File Write — Disk Exhaustion

**Title:** HIGH: fileWrite() creates unlimited directory depth and file size

**Labels:** `security`, `high`, `resource-exhaustion`, `file-access`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

The `fileWrite()` function in `file-handlers.ts` (lines 77-83) creates parent directories recursively without depth or size limits. Agents can fill the disk or create thousands of nested directories.

**Remediation:**

Add size and depth limits:

```typescript
const MAX_FILE_SIZE = 10 * 1024 * 1024;
const MAX_DIR_DEPTH = 20;

export function fileWrite(workspacePath: string, filePath: string, content: string): string {
  const resolved = resolveSafe(workspacePath, filePath);

  if (content.length > MAX_FILE_SIZE) {
    return JSON.stringify({
      error: `File exceeds maximum size of ${MAX_FILE_SIZE} bytes`,
    });
  }

  const relPath = path.relative(workspacePath, resolved);
  const depth = relPath.split(path.sep).length;
  if (depth > MAX_DIR_DEPTH) {
    return JSON.stringify({
      error: `Directory depth exceeds maximum of ${MAX_DIR_DEPTH}`,
    });
  }

  try {
    fs.mkdirSync(path.dirname(resolved), { recursive: true });
    fs.writeFileSync(resolved, content, 'utf-8');
    return JSON.stringify({ success: true });
  } catch (err) {
    return JSON.stringify({
      error: `Write failed: ${err instanceof Error ? err.message : String(err)}`,
    });
  }
}
```

---

## HIGH-006: Error Context Loss in Tool Execution

**Title:** HIGH: Tool execution errors logged generically without stack traces

**Labels:** `security`, `high`, `logging`, `debugging`

**Assignee:** [Developer name]

**Deadline:** 1 week

**Description:**

Tool execution errors in `loop.ts` (lines 854-882) are caught and logged with generic messages. Root cause context is lost, making debugging impossible.

**Remediation:**

Preserve error chain and include stack traces:

```typescript
try {
  results = await executeToolCalls(toolCalls, workspace, options);
} catch (err) {
  const errorContext = {
    message: err instanceof Error ? err.message : String(err),
    stack: err instanceof Error ? err.stack : undefined,
    toolCount: toolCalls.length,
  };
  log('error', `Tool execution failed: ${JSON.stringify(errorContext)}`);
  return {
    status: 'error',
    message: 'Tool execution failed',
    errorContext,
  };
}
```

---

## DEPENDENCY-001: picomatch ReDoS Vulnerabilities

**Title:** Update picomatch to fix ReDoS vulnerabilities (HIGH + MODERATE)

**Labels:** `security`, `dependencies`, `high`

**Assignee:** [Developer name]

**Deadline:** Before container image release

**Description:**

Transitive dependency picomatch >=4.0.0 <4.0.4 has two vulnerabilities:

1. **HIGH:** ReDoS vulnerability via extglob quantifiers (GHSA-c2c7-rcm5-vvqj)
2. **MODERATE:** Method injection in POSIX character classes (GHSA-3v7f-55p6-f55p)

Path: vitest → vite → tinyglobby → fdir → picomatch

**Fix:**

```bash
bun update picomatch
```

Verify version is >=4.0.4:

```bash
bun list picomatch
```

---

# Filing Instructions

1. Copy each issue template above
2. Create a GitHub issue for each finding
3. Adjust the title, assignee, and deadline as needed
4. Use the provided code examples for implementation guidance
5. Link all issues to the security audit for traceability
6. Track completion against the Phase 1/2/3 timeline

---

**Report Generated:** 2026-04-04  
**Security Audit:** D:/projects/homelab/sera/SECURITY_AUDIT_AGENT_RUNTIME.md
