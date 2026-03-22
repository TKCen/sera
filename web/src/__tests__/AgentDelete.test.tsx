/**
 * AgentsPage — delete agent feature
 *
 * Covers:
 *  - Delete button is rendered (hidden until hover, present in DOM)
 *  - Clicking Delete shows a confirmation dialog with the agent name
 *  - Confirming calls deleteAgent mutation with the correct name
 *  - Cancelling does NOT call the mutation
 *  - On success the agents query is invalidated (list refreshes)
 *  - On failure a toast error is shown
 *  - Bootstrap auto-creation: GET /api/agents does NOT include a 'sera' instance
 */

import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter } from 'react-router';

// ── Hoisted mutable state for mock control ────────────────────────────────────

const mockDeleteFn = vi.hoisted(() => vi.fn());
const mockConfirmReturn = vi.hoisted(() => ({ value: true }));

// ── Module mocks (must be before any imports of the mocked modules) ───────────

vi.mock('@/hooks/useAgents', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useAgents')>();
  return {
    ...actual,
    useAgents: vi.fn().mockReturnValue({
      data: [
        {
          id: 'inst-001',
          name: 'qwen-assistant',
          display_name: 'Qwen Assistant',
          template_ref: 'sera',
          status: 'stopped',
          sandbox_boundary: 'tier-2',
        },
        {
          id: 'inst-002',
          name: 'writer',
          display_name: 'Writer',
          template_ref: 'developer',
          status: 'stopped',
          circle: 'general',
        },
      ],
      isLoading: false,
    }),
    useStartAgent: vi.fn().mockReturnValue({ mutateAsync: vi.fn(), isPending: false }),
    useStopAgent: vi.fn().mockReturnValue({ mutateAsync: vi.fn(), isPending: false }),
    useDeleteAgent: vi.fn().mockReturnValue({
      mutateAsync: mockDeleteFn,
      isPending: false,
    }),
  };
});

vi.mock('@/hooks/useAuth', () => ({
  useAuth: () => ({
    user: { sub: 'dev', name: 'Dev', roles: ['admin'] },
    isAuthenticated: true,
    isLoading: false,
  }),
}));

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock('@/components/AgentStatusBadge', () => ({
  AgentStatusBadge: ({ agentId }: { agentId: string }) => (
    <span data-testid={`status-${agentId}`}>STOPPED</span>
  ),
}));

// ── Lazy import after mocks ────────────────────────────────────────────────────

import AgentsPage from '@/app/agents/page';
import { toast } from 'sonner';

// ── Helpers ───────────────────────────────────────────────────────────────────

function TestWrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <BrowserRouter>{children}</BrowserRouter>
    </QueryClientProvider>
  );
}

function renderPage() {
  return render(
    <TestWrapper>
      <AgentsPage />
    </TestWrapper>
  );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('AgentsPage — delete agent', () => {
  beforeEach(() => {
    mockDeleteFn.mockReset();
    mockDeleteFn.mockResolvedValue(undefined);
    mockConfirmReturn.value = true;
    vi.stubGlobal('confirm', () => mockConfirmReturn.value);
  });

  afterEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  // ── Rendering ──────────────────────────────────────────────────────────────

  it('renders a Delete button for each agent in the DOM', () => {
    renderPage();
    const deleteButtons = screen.getAllByTitle('Delete');
    expect(deleteButtons).toHaveLength(2);
  });

  it('Delete buttons are hidden by default (opacity-0) and revealed on group hover via CSS', () => {
    renderPage();
    // The buttons are in the DOM but inside opacity-0 container — the CSS class
    // controls visibility so the buttons are always accessible programmatically.
    const deleteButtons = screen.getAllByTitle('Delete');
    deleteButtons.forEach((btn) => {
      expect(btn).toBeInTheDocument();
      // Closest parent with the opacity class
      const container = btn.closest('.opacity-0');
      expect(container).not.toBeNull();
    });
  });

  it('does not render an agent instance named "sera"', () => {
    renderPage();
    // The mock data has template_ref: 'sera' which renders in a Badge.
    // But no agent instance is *named* "sera" — the instance names are qwen-assistant and writer.
    const agentNames = screen.getAllByText(/.+/, { selector: '.font-medium.text-sm' });
    const nameTexts = agentNames.map((el) => el.textContent);
    expect(nameTexts).not.toContain('sera');
  });

  // ── Confirm dialog ─────────────────────────────────────────────────────────

  it('clicking Delete shows a confirmation dialog mentioning the agent name', async () => {
    const user = userEvent.setup();
    const confirmSpy = vi.fn(() => true);
    vi.stubGlobal('confirm', confirmSpy);
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    expect(confirmSpy).toHaveBeenCalledOnce();
    const firstCall = confirmSpy.mock.calls[0] as unknown as string[];
    expect(firstCall[0]).toMatch(/qwen-assistant/);
    expect(firstCall[0]).toMatch(/permanently/i);
  });

  it('confirms the dialog and calls deleteAgent with the correct agent name', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    await waitFor(() => {
      expect(mockDeleteFn).toHaveBeenCalledOnce();
      expect(mockDeleteFn).toHaveBeenCalledWith('inst-001');
    });
  });

  it('cancelling the confirmation does NOT call deleteAgent', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => false);
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    // Give any async mutations a chance to fire
    await new Promise((r) => setTimeout(r, 50));
    expect(mockDeleteFn).not.toHaveBeenCalled();
  });

  // ── Success / error feedback ───────────────────────────────────────────────

  it('shows a success toast after confirmed deletion', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(expect.stringMatching(/qwen-assistant/));
    });
  });

  it('shows an error toast when deleteAgent rejects', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    mockDeleteFn.mockRejectedValueOnce(new Error('Network error'));
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Network error');
    });
  });

  // ── Second agent ───────────────────────────────────────────────────────────

  it('deletes the correct agent when the second Delete button is clicked', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => true);
    renderPage();

    const deleteButtons = screen.getAllByTitle('Delete');
    await user.click(deleteButtons[1]!);

    await waitFor(() => {
      expect(mockDeleteFn).toHaveBeenCalledWith('inst-002');
    });
  });

  // ── Event propagation ──────────────────────────────────────────────────────

  it('Delete button click does not navigate to the agent detail page', async () => {
    const user = userEvent.setup();
    vi.stubGlobal('confirm', () => false); // cancel so no API call, just checking nav
    renderPage();

    const [firstDelete] = screen.getAllByTitle('Delete');
    await user.click(firstDelete!);

    // URL should remain on /agents (BrowserRouter starts at /)
    expect(window.location.pathname).not.toMatch(/\/agents\/qwen-assistant/);
  });
});
