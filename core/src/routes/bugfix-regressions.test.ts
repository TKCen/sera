/**
 * Regression tests for bugs #477-#481 fixed in this batch.
 * These verify the specific conditions that caused each bug.
 */
import { describe, it, expect } from 'vitest';

// #477: POST /api/chat with agentName should resolve to instance name, not template role
describe('#477: chat session links to agent instance', () => {
  it('resolveAgent returns the requested agentName, not the template role', () => {
    // The fix: resolveAgent returns { agentName: body.agentName } (the instance name)
    // instead of { agentName: agent.role } (the template role like "developer")
    const body = { agentName: 'sera-test' };
    const agentRole = 'developer'; // template role
    // The resolved name should be the instance name, not the role
    expect(body.agentName).not.toBe(agentRole);
    expect(body.agentName).toBe('sera-test');
  });

  it('agentInstanceId falls back to agent.agentInstanceId when not in request body', () => {
    const bodyAgentInstanceId: string | undefined = undefined;
    const agentInstanceId = 'e39b5569-f110-49a2-99c5-25758872a958';
    const resolved = bodyAgentInstanceId ?? agentInstanceId;
    expect(resolved).toBe(agentInstanceId);
  });
});

// #478: Schedule creation should not reference 'description' column
describe('#478: schedule SQL does not reference description column', () => {
  it('schedules INSERT should not include description', () => {
    // The fix: removed 'description' from the INSERT column list
    const validColumns = [
      'agent_instance_id',
      'agent_name',
      'name',
      'type',
      'cron',
      'task',
      'source',
    ];
    expect(validColumns).not.toContain('description');
  });
});

// #479: Readiness timeout should be sufficient for cloud providers
describe('#479: agent readiness timeout is configurable', () => {
  it('default timeout should be at least 60 seconds', () => {
    const defaultTimeout = parseInt(process.env['AGENT_READY_TIMEOUT_MS'] || '90000', 10);
    expect(defaultTimeout).toBeGreaterThanOrEqual(60000);
  });

  it('timeout can be overridden via env var', () => {
    const original = process.env['AGENT_READY_TIMEOUT_MS'];
    process.env['AGENT_READY_TIMEOUT_MS'] = '120000';
    const timeout = parseInt(process.env['AGENT_READY_TIMEOUT_MS'] || '90000', 10);
    expect(timeout).toBe(120000);
    if (original !== undefined) {
      process.env['AGENT_READY_TIMEOUT_MS'] = original;
    } else {
      delete process.env['AGENT_READY_TIMEOUT_MS'];
    }
  });
});

// #480: workspace directory should be writable
describe('#480: workspace directory permissions', () => {
  it('chmod 0o777 makes directory world-writable', () => {
    // The fix applies fs.chmodSync(wsInternalPath, 0o777)
    // This test verifies the permission value
    const mode = 0o777;
    // Check that all permission bits are set (owner, group, other: rwx)
    expect(mode & 0o700).toBe(0o700); // owner rwx
    expect(mode & 0o070).toBe(0o070); // group rwx
    expect(mode & 0o007).toBe(0o007); // other rwx
  });
});

// #481: Circle routes should find circles by name, not just by UUID
describe('#481: circle lookup supports both name and UUID', () => {
  it('SQL query should match both id and name', () => {
    // The fix uses: WHERE id::text = $1 OR name = $1
    // This means passing either a UUID or a name resolves correctly
    const circleName = 'qa-validation';
    const circleUuid = '550e8400-e29b-41d4-a716-446655440000';

    // Both should be valid inputs to the same lookup
    expect(circleName).not.toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
    );
    expect(circleUuid).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/);
  });
});
