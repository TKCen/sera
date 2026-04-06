/**
 * Threat pattern scanning for shell commands.
 *
 * Ported from Goose's categorized regex threat pattern library:
 * goose/crates/goose/src/security/patterns.rs
 *
 * Scans commands before execution and returns matches with risk levels.
 * Callers should block Critical/High matches and warn on Medium.
 */

// ── Risk Levels ──────────────────────────────────────────────────────────────

export type RiskLevel = 'Low' | 'Medium' | 'High' | 'Critical';

// ── Pattern Definition ───────────────────────────────────────────────────────

export interface ThreatPattern {
  /** Human-readable name for this specific pattern */
  name: string;
  /** Regex that matches the dangerous command/construct */
  pattern: RegExp;
  /** Short explanation of what this pattern detects */
  description: string;
}

// ── Category Definition ──────────────────────────────────────────────────────

export interface ThreatCategory {
  /** Category identifier (e.g. "FileSystemDestruction") */
  name: string;
  /** Human-readable description of the threat category */
  description: string;
  riskLevel: RiskLevel;
  patterns: ThreatPattern[];
}

// ── Match Result ─────────────────────────────────────────────────────────────

export interface ThreatMatch {
  category: string;
  patternName: string;
  riskLevel: RiskLevel;
  description: string;
  /** Approximate confidence that this is a true positive (0-1) */
  confidence: number;
  /** The portion of the command that matched */
  matchedText: string;
}

// ── Threat Categories ────────────────────────────────────────────────────────

const THREAT_CATEGORIES: ThreatCategory[] = [
  {
    name: 'FileSystemDestruction',
    description: 'Commands that can destroy file system data irreversibly',
    riskLevel: 'Critical',
    patterns: [
      {
        name: 'rm_rf_root',
        pattern: /\brm\s+(?:-[a-zA-Z]*r[a-zA-Z]*f|--recursive\s+--force|-rf|-fr)\s+\/(?:\s|$)/,
        description: 'Recursive force-delete from filesystem root',
      },
      {
        name: 'rm_rf_home',
        pattern: /\brm\s+(?:-[a-zA-Z]*r[a-zA-Z]*f|--recursive\s+--force|-rf|-fr)\s+~(?:\/|\s|$)/,
        description: 'Recursive force-delete from home directory',
      },
      {
        name: 'dd_overwrite_disk',
        pattern: /\bdd\b.*\bof=\/dev\/(?:sd[a-z]|hd[a-z]|nvme\d|vd[a-z]|xvd[a-z])\b/,
        description: 'Direct disk overwrite via dd',
      },
      {
        name: 'mkfs_format_disk',
        pattern: /\bmkfs(?:\.[a-z0-9]+)?\s+\/dev\//,
        description: 'Format block device with mkfs',
      },
      {
        name: 'shred_disk',
        pattern: /\bshred\b.*\/dev\/(?:sd[a-z]|hd[a-z]|nvme\d)/,
        description: 'Shred entire block device',
      },
      {
        name: 'wipefs',
        pattern: /\bwipefs\s+-a\s+\/dev\//,
        description: 'Wipe filesystem signatures from block device',
      },
      {
        name: 'truncate_etc',
        pattern: /\btruncate\b.*\s+\/etc\//,
        description: 'Truncate system configuration files',
      },
    ],
  },

  {
    name: 'RemoteCodeExecution',
    description: 'Commands that fetch and execute code from remote sources',
    riskLevel: 'Critical',
    patterns: [
      {
        name: 'curl_pipe_sh',
        pattern: /\bcurl\b.*\|\s*(?:ba)?sh\b/,
        description: 'Pipe curl output directly into shell',
      },
      {
        name: 'wget_pipe_sh',
        pattern: /\bwget\b.*\|\s*(?:ba)?sh\b/,
        description: 'Pipe wget output directly into shell',
      },
      {
        name: 'curl_exec',
        pattern: /\bcurl\b.*-[sSkL]*o\s+.*&&\s*(?:ba)?sh\b/,
        description: 'Download file and immediately execute with shell',
      },
      {
        name: 'eval_remote',
        pattern: /\beval\s+\$\((?:curl|wget|fetch)\b/,
        description: 'eval of remote command substitution output',
      },
      {
        name: 'python_exec_remote',
        pattern: /\bpython3?\s+-c\s+["'].*(?:urllib|requests|http).*exec\b/,
        description: 'Python inline execution of remotely fetched code',
      },
      {
        name: 'bash_process_substitution_remote',
        pattern: /\bbash\s+<\s*\(\s*(?:curl|wget)\b/,
        description: 'Execute remote script via bash process substitution',
      },
    ],
  },

  {
    name: 'DataExfiltration',
    description: 'Commands that may transmit sensitive data to external endpoints',
    riskLevel: 'High',
    patterns: [
      {
        name: 'curl_post_shadow',
        pattern: /\bcurl\b.*-d\s+.*\/etc\/(?:shadow|passwd)/,
        description: 'POST /etc/shadow or /etc/passwd contents via curl',
      },
      {
        name: 'curl_post_ssh_key',
        pattern: /\bcurl\b.*-d\s+.*\.ssh\/(?:id_rsa|id_ed25519|id_ecdsa)/,
        description: 'POST SSH private key via curl',
      },
      {
        name: 'netcat_exfil',
        pattern: /\bnc\b.*-[a-zA-Z]*e\s+.*(?:\/bin\/(?:ba)?sh|cat\s+\/)/,
        description: 'Netcat exfiltration or reverse shell',
      },
      {
        name: 'env_to_remote',
        pattern: /\benv\b.*\|\s*(?:curl|wget|nc)\b/,
        description: 'Pipe environment variables to remote endpoint',
      },
      {
        name: 'aws_creds_exfil',
        pattern: /(?:AWS_SECRET_ACCESS_KEY|AWS_ACCESS_KEY_ID).*\|\s*(?:curl|wget|nc)\b/,
        description: 'Send AWS credentials to remote endpoint',
      },
      {
        name: 'history_exfil',
        pattern: /\bhistory\b.*\|\s*(?:curl|wget|nc)\b/,
        description: 'Exfiltrate shell command history',
      },
    ],
  },

  {
    name: 'SystemModification',
    description: 'Commands that make persistent or destructive system-level changes',
    riskLevel: 'High',
    patterns: [
      {
        name: 'crontab_overwrite',
        pattern: /\bcrontab\s+-[rl].*>\s*\/|echo\s+.*>\s*\/etc\/cron/,
        description: 'Overwrite crontab or system cron files',
      },
      {
        name: 'write_etc_passwd',
        pattern: />\s*\/etc\/passwd/,
        description: 'Overwrite /etc/passwd',
      },
      {
        name: 'write_etc_shadow',
        pattern: />\s*\/etc\/shadow/,
        description: 'Overwrite /etc/shadow',
      },
      {
        name: 'iptables_flush',
        pattern: /\biptables\s+-F\b/,
        description: 'Flush all iptables rules (disable firewall)',
      },
      {
        name: 'systemctl_disable_security',
        pattern:
          /\bsystemctl\s+(?:disable|stop|mask)\s+(?:firewalld|ufw|apparmor|selinux|auditd)\b/,
        description: 'Disable security daemon via systemctl',
      },
      {
        name: 'setenforce_permissive',
        pattern: /\bsetenforce\s+0\b/,
        description: 'Set SELinux to permissive mode',
      },
      {
        name: 'disable_apparmor',
        pattern: /\baa-complain\b|\baa-disable\b/,
        description: 'Disable or complain-mode AppArmor profile',
      },
      {
        name: 'write_ssh_authorized_keys',
        pattern: />\s*~?\/\.ssh\/authorized_keys/,
        description: 'Overwrite SSH authorized_keys file',
      },
    ],
  },

  {
    name: 'PrivilegeEscalation',
    description: 'Commands that attempt to gain elevated privileges',
    riskLevel: 'High',
    patterns: [
      {
        name: 'sudo_all',
        pattern: /\becho\s+["'].*ALL.*NOPASSWD.*ALL["']\s*(?:>>|>)\s*\/etc\/sudoers/,
        description: 'Add passwordless sudo rule to /etc/sudoers',
      },
      {
        name: 'sudoers_nopasswd',
        pattern: /NOPASSWD\s*:\s*ALL/,
        description: 'NOPASSWD:ALL sudoers directive',
      },
      {
        name: 'chmod_suid',
        pattern: /\bchmod\s+(?:[2467][0-7]{3}|[ugo][+]=s)\s+/,
        description: 'Set SUID/SGID bit on a file',
      },
      {
        name: 'chown_root_bash',
        pattern: /\bchown\s+root\s+\/(?:bin\/bash|usr\/bin\/bash)\b/,
        description: 'Change bash ownership to root',
      },
      {
        name: 'nsenter_host',
        pattern: /\bnsenter\s+(?:-[a-zA-Z]*t\s+1|--target\s+1)\b/,
        description: 'Enter host namespace (container escape)',
      },
      {
        name: 'docker_privileged_escape',
        pattern: /\bdocker\s+run\s+(?:.*\s+)?--privileged\b/,
        description: 'Run privileged container (potential host escape)',
      },
      {
        name: 'useradd_no_password',
        pattern: /\buseradd\b(?!.*-p\s+\S)/,
        description: 'Create user account without password (may allow backdoor)',
      },
    ],
  },

  {
    name: 'CommandInjection',
    description: 'Patterns that indicate command injection or obfuscation attempts',
    riskLevel: 'Medium',
    patterns: [
      {
        name: 'base64_decode_exec',
        pattern: /\bbase64\s+(?:-d|--decode)\s*.*\|\s*(?:ba)?sh\b/,
        description: 'Decode base64 and pipe into shell',
      },
      {
        name: 'hex_decode_exec',
        pattern: /\bprintf\s+['"]\\x[0-9a-fA-F]+.*\|\s*(?:ba)?sh\b/,
        description: 'Decode hex-escaped payload and execute',
      },
      {
        name: 'subshell_obfuscation',
        pattern: /\$'\\./,
        description: 'ANSI-C quoting to obfuscate command characters',
      },
      {
        name: 'ifs_manipulation',
        pattern: /\bIFS\s*=\s*['"]?.['"]?\s+/,
        description: 'IFS manipulation to split/obfuscate commands',
      },
      {
        name: 'python_os_system',
        pattern: /\bpython3?\s+-c\s+["'].*os\.system\s*\(/,
        description: 'Inline Python executing os.system()',
      },
      {
        name: 'perl_exec',
        pattern: /\bperl\s+-e\s+["'].*system\s*\(/,
        description: 'Inline Perl executing system()',
      },
      {
        name: 'reverse_shell_bash',
        pattern: /\bbash\s+-i\s+>&?\s*\/dev\/tcp\//,
        description: 'Bash TCP reverse shell',
      },
      {
        name: 'reverse_shell_python',
        pattern: /\bpython3?\s+-c\s+["'].*socket.*connect\s*\(.*\)\s*.*exec\b/,
        description: 'Python reverse shell via socket',
      },
    ],
  },
];

// ── Confidence by Risk Level ─────────────────────────────────────────────────

const CONFIDENCE_BY_RISK: Record<RiskLevel, number> = {
  Critical: 0.95,
  High: 0.85,
  Medium: 0.7,
  Low: 0.5,
};

// ── Scanner ──────────────────────────────────────────────────────────────────

/**
 * Scan a shell command string for known threat patterns.
 *
 * Returns all matches found. An empty array means no threats detected.
 * Callers should block execution when any match has riskLevel Critical or High,
 * and emit a warning for Medium.
 */
export function scanCommand(command: string): ThreatMatch[] {
  const matches: ThreatMatch[] = [];

  for (const category of THREAT_CATEGORIES) {
    for (const threat of category.patterns) {
      const match = threat.pattern.exec(command);
      if (match !== null) {
        matches.push({
          category: category.name,
          patternName: threat.name,
          riskLevel: category.riskLevel,
          description: threat.description,
          confidence: CONFIDENCE_BY_RISK[category.riskLevel],
          matchedText: match[0],
        });
      }
    }
  }

  return matches;
}

/**
 * Returns true if any match should block execution (Critical or High risk).
 */
export function shouldBlock(matches: ThreatMatch[]): boolean {
  return matches.some((m) => m.riskLevel === 'Critical' || m.riskLevel === 'High');
}

/**
 * Returns true if any match warrants a warning but not a block (Medium risk).
 */
export function hasWarnings(matches: ThreatMatch[]): boolean {
  return matches.some((m) => m.riskLevel === 'Medium');
}
