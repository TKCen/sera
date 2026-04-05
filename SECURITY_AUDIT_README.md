# SERA Agent-Runtime Security Audit Report

**Date:** 2026-04-04  
**Risk Level:** CRITICAL  
**Status:** Complete — 16 vulnerabilities identified, 4 CRITICAL, 6 HIGH, 4 MEDIUM, 2 LOW

---

## Documents

### 1. Executive Summary (Quick Reference)

**File:** `SECURITY_AUDIT_SUMMARY.txt` (207 lines)

Quick overview of all findings organized by severity. Read this first for a high-level understanding of risks, blocking issues, and remediation sequence.

**Contents:**

- Critical issues requiring immediate fix (blocks production)
- High-priority issues (1-week deadline)
- Medium-priority issues (1-month deadline)
- Dependency vulnerabilities
- OWASP Top 10 coverage matrix
- Recommended remediation sequence (Phases 1-3)
- Files affected summary

---

### 2. Detailed Technical Report

**File:** `SECURITY_AUDIT_AGENT_RUNTIME.md` (1,092 lines)

Comprehensive security audit with technical deep-dive for each vulnerability. Includes code examples, secure remediation with working code, exploitability assessment, and blast radius analysis.

**Contents:**

- 4 CRITICAL issues with secure code examples
- 6 HIGH issues with remediation guidance
- 4 MEDIUM issues with implementation details
- 2 LOW issues
- Dependency audit results (picomatch CVEs)
- Secrets scan results (PASSED)
- Vulnerability prioritization matrix (16 items)
- Immediate action items by phase
- Security checklist (OWASP Top 10)
- Assessment summary

---

## Key Findings Summary

### Critical Issues (Blocks Production)

| #   | Issue                         | File                            | Line    | Severity |
| --- | ----------------------------- | ------------------------------- | ------- | -------- |
| 1   | Code Execution Sandbox Escape | code-handler.ts                 | 62      | CRITICAL |
| 2   | Incomplete SSRF Filtering     | http-handler.ts, web-handler.ts | 19-22   | CRITICAL |
| 3   | Symlink Path Traversal        | file-handlers.ts                | 143-151 | CRITICAL |
| 4   | Shell Path Regex Bypass       | shell-handler.ts                | 213     | CRITICAL |

### High Priority Issues (1-Week Deadline)

| #   | Issue                     | File               | Severity |
| --- | ------------------------- | ------------------ | -------- |
| 5   | Ripgrep Pattern Injection | search-handlers.ts | HIGH     |
| 6   | Silent Error Swallowing   | index.ts           | HIGH     |
| 7   | Semaphore Deadlock        | executor.ts        | HIGH     |
| 8   | Missing Token Validation  | proxy.ts           | HIGH     |
| 9   | Unbounded File Write      | file-handlers.ts   | HIGH     |
| 10  | Error Context Loss        | loop.ts            | HIGH     |

---

## Remediation Roadmap

### Phase 1: CRITICAL Fixes (Do Immediately)

1. Replace code sandbox with isolated-vm
2. Fix SSRF filtering — comprehensive IP blocklist
3. Use fs.realpathSync() for symlink-safe path validation
4. Replace shell path regex with pattern blocklist
5. Update picomatch to >=4.0.4

**Estimated Effort:** 8-12 hours  
**Blocking:** Yes — cannot deploy without these fixes

### Phase 2: HIGH Fixes (Within 1 Week)

6-10: Ripgrep injection, poll loop backoff, semaphore timeout, token validation, file write limits

**Estimated Effort:** 6-8 hours  
**Blocking:** Operational safety

### Phase 3: MEDIUM Fixes (Within 1 Month)

11-14: Streaming grep, resource cleanup, output truncation, rate limiting

**Estimated Effort:** 4-6 hours  
**Blocking:** No, but improves robustness

---

## Scope

**Files Audited:** 11 TypeScript files  
**Lines Reviewed:** ~1,500 lines  
**Categories:** Tool execution, shell commands, file I/O, path validation, HTTP requests, SSRF prevention, error handling, authentication

**Files:**

- `tools/executor.ts` (684 lines) — Tool execution dispatcher
- `tools/shell-handler.ts` (223 lines) — Shell command execution
- `tools/file-handlers.ts` (221 lines) — File I/O operations
- `tools/search-handlers.ts` (225 lines) — Glob and grep operations
- `tools/code-handler.ts` (99 lines) — Code evaluation sandbox
- `tools/http-handler.ts` (63 lines) — HTTP requests with SSRF prevention
- `tools/web-handler.ts` (106 lines) — Web fetch with streaming
- `tools/proxy.ts` (153 lines) — HTTP proxy to sera-core
- `loop.ts` (921 lines) — Reasoning loop with tool execution
- `index.ts` (381 lines) — Bootstrap and task polling
- `centrifugo.ts` (100+ lines) — Real-time messaging

---

## OWASP Top 10 Assessment

| Category                       | Result  | Notes                                                                                         |
| ------------------------------ | ------- | --------------------------------------------------------------------------------------------- |
| A01: Broken Access Control     | FAILED  | Path traversal via symlinks (Issue #3)                                                        |
| A02: Cryptographic Failures    | PASSED  | Secrets in env vars, no hardcoded keys                                                        |
| A03: Injection                 | FAILED  | Code eval sandbox broken (Issue #1), shell injection (Issue #4), pattern injection (Issue #5) |
| A04: Insecure Design           | PARTIAL | Missing resource cleanup strategy                                                             |
| A05: Security Misconfiguration | FAILED  | Incomplete SSRF (Issue #2), no rate limits (Issue #14)                                        |
| A06: Vulnerable Components     | FAILED  | 2 CVEs in picomatch transitive dependency                                                     |
| A07: Auth Failures             | FAILED  | No token validation (Issue #8), silent failures (Issue #6)                                    |
| A08: Data Integrity            | PARTIAL | Resource leaks (Issue #12), no cleanup guarantees                                             |
| A09: Logging Failures          | FAILED  | Silent error swallowing (Issue #6), no context (Issue #10)                                    |
| A10: SSRF                      | FAILED  | Incomplete IP filtering (Issue #2)                                                            |

**Score:** 3/10 PASSED (30%)

---

## Dependency Audit Results

**Status:** 2 vulnerabilities found

### HIGH Severity

- **Package:** picomatch >=4.0.0 <4.0.4
- **Issue:** ReDoS vulnerability via extglob quantifiers
- **Advisory:** GHSA-c2c7-rcm5-vvqj
- **Path:** vitest → vite → tinyglobby → fdir → picomatch
- **Fix:** `bun update picomatch`

### MODERATE Severity

- **Package:** picomatch >=4.0.0 <4.0.4
- **Issue:** Method injection in POSIX character classes
- **Advisory:** GHSA-3v7f-55p6-f55p
- **Path:** vitest → vite → tinyglobby → fdir → picomatch
- **Fix:** `bun update picomatch`

---

## Secrets Scan Results

**Status:** PASSED  
**Coverage:** All TypeScript files in core/agent-runtime/src/  
**Finding:** No hardcoded API keys, passwords, tokens, or credentials detected

**Verification:**

- Searched for: `api_key`, `password`, `secret`, `token`, `credential`, `private_key`
- Result: All matches are legitimate (token budget tracking, env var references, safe logging lists)
- Secrets correctly managed via environment variables

---

## Blocking Status

**PRODUCTION DEPLOYMENT: BLOCKED**

This codebase is **NOT SAFE** for production use without fixing Phase 1 critical issues:

1. Code execution sandbox escape — enables RCE
2. SSRF filtering incomplete — enables metadata service access
3. Path traversal via symlinks — enables arbitrary file access
4. Shell command injection — enables arbitrary command execution
5. Silent auth failures — masks security violations

**Recommendation:** Implement Phase 1 fixes before any production deployment. Re-audit after fixes before releasing container image.

---

## How to Use These Reports

### For Security Team

1. Read `SECURITY_AUDIT_SUMMARY.txt` for overview
2. Review vulnerability prioritization matrix
3. Assign Phase 1 fixes to dev team with 24-hour deadline
4. Track remediation progress against Phase roadmap

### For Development Team

1. Start with `SECURITY_AUDIT_SUMMARY.txt` for issue list
2. Open `SECURITY_AUDIT_AGENT_RUNTIME.md` for detailed remediation
3. Follow the "Remediation:" section in each issue for working code examples
4. Test fixes and request re-audit before merge

### For Release Manager

1. Do NOT release container image until Phase 1 complete
2. Verify Phase 1 fixes are implemented and tested
3. Request security re-audit before production release
4. Document Phase 2/3 fixes in release notes as improvements

---

## Next Steps

1. **File GitHub issues** for each CRITICAL and HIGH vulnerability
2. **Assign Phase 1 fixes** to development team (24-hour deadline)
3. **Implement fixes** using code examples from detailed report
4. **Test remediation** with unit and integration tests
5. **Schedule re-audit** after fixes complete
6. **Document decisions** in GitHub issue comments

---

## Contact & Questions

For questions about specific findings, refer to the detailed technical report with line numbers and code examples. Each issue includes:

- Exact file path and line number
- Vulnerable code snippet
- Secure code example (working implementation)
- Exploitability assessment
- Blast radius analysis
- Remediation priority and deadline

---

**Report Generated:** 2026-04-04  
**Auditor:** Security Reviewer (Claude Code Agent)  
**Audit Methodology:** OWASP Top 10 + CWE Analysis + Manual Code Review
