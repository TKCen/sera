import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AgentStatusBadge } from './AgentStatusBadge';
import { useAgentStatus } from '@/hooks/useAgentStatus';

vi.mock('@/hooks/useAgentStatus');

describe('AgentStatusBadge', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders live status if available', () => {
    vi.mocked(useAgentStatus).mockReturnValue('running');
    render(<AgentStatusBadge agentId="agent-1" />);
    expect(screen.getByText('running')).toBeDefined();
    expect(screen.getByText('running').className).toContain('text-sera-success');
  });

  it('renders static status if live status is not available', () => {
    vi.mocked(useAgentStatus).mockReturnValue(null);
    render(<AgentStatusBadge agentId="agent-1" staticStatus="error" />);
    expect(screen.getByText('error')).toBeDefined();
    expect(screen.getByText('error').className).toContain('text-sera-error');
  });

  it('renders "stopped" if no status is available', () => {
    vi.mocked(useAgentStatus).mockReturnValue(null);
    render(<AgentStatusBadge agentId="agent-1" />);
    expect(screen.getByText('stopped')).toBeDefined();
  });

  it('applies "unresponsive" styling correctly', () => {
    vi.mocked(useAgentStatus).mockReturnValue('unresponsive');
    render(<AgentStatusBadge agentId="agent-1" />);
    expect(screen.getByText('unresponsive')).toBeDefined();
    expect(screen.getByText('unresponsive').className).toContain('text-sera-warning');
  });
});
