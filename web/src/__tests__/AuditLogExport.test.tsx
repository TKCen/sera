import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { BrowserRouter } from 'react-router';

// Must be hoisted so vi.mock factory can reference it
const mockRoles = vi.hoisted(() => ({ value: ['admin'] as string[] }));

vi.mock('@/contexts/AuthContext', () => ({
  useAuth: () => ({
    roles: mockRoles.value,
    user: { sub: 'dev', name: 'Dev', roles: mockRoles.value },
    isAuthenticated: true,
    isLoading: false,
  }),
}));

vi.mock('@/lib/api/audit', () => ({
  getAuditEvents: vi.fn().mockResolvedValue({
    events: [
      {
        id: 'evt-1',
        sequence: 1,
        timestamp: new Date().toISOString(),
        actorId: 'agent-a',
        actorType: 'agent',
        actorName: 'agent-a',
        eventType: 'llm.request',
        resourceType: 'agent',
        resourceId: 'agent-a',
        status: 'success',
        payload: { model: 'gpt-4', tokens: 100 },
      },
    ],
    total: 1,
    page: 1,
    pageSize: 50,
  }),
  verifyAuditChain: vi.fn().mockResolvedValue({ valid: true, checkedCount: 42 }),
  getAuditExportUrl: vi.fn().mockReturnValue('/audit/export?format=jsonl'),
}));

// Static import after mocks are declared (Vitest hoists vi.mock above imports)
import AuditPage from '@/pages/AuditPage';

function TestWrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <BrowserRouter>{children}</BrowserRouter>
    </QueryClientProvider>
  );
}

// Build a large NDJSON response split into small chunks
function buildStreamChunks(lines: number, chunkSize = 256): Uint8Array[] {
  const rows = Array.from({ length: lines }, (_, i) =>
    JSON.stringify({ id: `evt-${i}`, sequence: i, eventType: 'test.event' })
  );
  const full = new TextEncoder().encode(rows.join('\n'));
  const chunks: Uint8Array[] = [];
  for (let offset = 0; offset < full.length; offset += chunkSize) {
    chunks.push(full.slice(offset, offset + chunkSize));
  }
  return chunks;
}

describe('AuditPage — streaming export', () => {
  const createObjectURL = vi.fn(() => 'blob:test-url');
  const revokeObjectURL = vi.fn();

  beforeEach(() => {
    mockRoles.value = ['admin'];
    // Stub URL methods used by the export handler
    vi.stubGlobal('URL', { createObjectURL, revokeObjectURL });
  });

  afterEach(() => {
    // clearAllMocks resets call counts without destroying mock implementations
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it('renders audit page with event table for admin', async () => {
    render(
      <TestWrapper>
        <AuditPage />
      </TestWrapper>
    );

    await waitFor(() => expect(screen.getByText('Audit Log')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText('llm.request')).toBeInTheDocument());
  });

  it('shows 403 state for non-admin user', () => {
    mockRoles.value = ['viewer'];

    render(
      <TestWrapper>
        <AuditPage />
      </TestWrapper>
    );

    expect(screen.getByText('Access Restricted')).toBeInTheDocument();
  });

  it('streams export response using ReadableStream reader pattern', async () => {
    // Build mock streaming fetch — many small chunks to verify streaming, not single read
    const chunks = buildStreamChunks(100);
    let readIdx = 0;
    const mockReadFn = vi.fn().mockImplementation(async () => {
      if (readIdx >= chunks.length) return { done: true as const, value: undefined };
      return { done: false as const, value: chunks[readIdx++] };
    });

    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        body: { getReader: () => ({ read: mockReadFn }) },
      })
    );

    render(
      <TestWrapper>
        <AuditPage />
      </TestWrapper>
    );

    await waitFor(() => screen.getByText('Audit Log'));

    // Track anchor element click for the download trigger
    const clickSpy = vi.fn();
    const origCreateElement = document.createElement.bind(document);
    const createElementSpy = vi
      .spyOn(document, 'createElement')
      .mockImplementation((tag: string) => {
        const el = origCreateElement(tag);
        if (tag === 'a') vi.spyOn(el as HTMLAnchorElement, 'click').mockImplementation(clickSpy);
        return el;
      });

    fireEvent.click(screen.getByText('Export'));

    await waitFor(() => {
      expect(createObjectURL).toHaveBeenCalledOnce();
    });

    // The argument to createObjectURL must be a Blob (assembled from stream chunks)
    const calls = createObjectURL.mock.calls as unknown as Array<[Blob]>;
    expect(calls[0][0]).toBeInstanceOf(Blob);

    // reader.read() was called once per chunk plus the final done=true call
    expect(mockReadFn.mock.calls.length).toBeGreaterThan(1);

    createElementSpy.mockRestore();
  });

  it('shows chain integrity result after verify', async () => {
    render(
      <TestWrapper>
        <AuditPage />
      </TestWrapper>
    );

    await waitFor(() => screen.getByText('Verify Chain'));
    fireEvent.click(screen.getByText('Verify Chain'));

    await waitFor(() => {
      expect(screen.getByText(/Chain integrity verified/)).toBeInTheDocument();
    });
    expect(screen.getByText(/42 events checked/)).toBeInTheDocument();
  });

  it('expands event row to show payload JSON', async () => {
    render(
      <TestWrapper>
        <AuditPage />
      </TestWrapper>
    );

    await waitFor(() => expect(screen.getByText('llm.request')).toBeInTheDocument());

    fireEvent.click(screen.getByText('llm.request'));

    await waitFor(() => {
      expect(screen.getByText(/"model"/)).toBeInTheDocument();
    });
  });
});
