import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useAgentStatus } from './useAgentStatus';
import { useChannel } from './useChannel';

vi.mock('./useChannel');

describe('useAgentStatus', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should return null if agentId is an empty string', () => {
    const { result } = renderHook(() => useAgentStatus(''));
    expect(useChannel).toHaveBeenCalledWith('');
    expect(result.current).toBeNull();
  });

  it('should return null if useChannel returns null', () => {
    vi.mocked(useChannel).mockReturnValue(null);
    const { result } = renderHook(() => useAgentStatus('agent-123'));
    expect(useChannel).toHaveBeenCalledWith('agent:agent-123:status');
    expect(result.current).toBeNull();
  });

  it('should return the status from the channel payload', () => {
    vi.mocked(useChannel).mockReturnValue({
      status: 'running',
      agentId: 'agent-123',
      timestamp: '2023-01-01T00:00:00Z',
    });
    const { result } = renderHook(() => useAgentStatus('agent-123'));
    expect(result.current).toBe('running');
  });

  it('should return null if payload exists but status is missing', () => {
    vi.mocked(useChannel).mockReturnValue({
      agentId: 'agent-123',
    } as any);
    const { result } = renderHook(() => useAgentStatus('agent-123'));
    expect(result.current).toBeNull();
  });
});
