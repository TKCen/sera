import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Subscription, Centrifuge, PublicationContext } from 'centrifuge';
import { useSystemEvents, type SystemEvent } from './useSystemEvents';
import { useCentrifugoContext } from '@/hooks/useCentrifugo';

vi.mock('@/hooks/useCentrifugo');

describe('useSystemEvents', () => {
  let mockSubscription: Subscription;
  let mockClient: Centrifuge;
  let publicationCallback: ((ctx: PublicationContext) => void) | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
    publicationCallback = undefined;

    mockSubscription = {
      on: vi.fn((event: string, cb: (ctx: PublicationContext) => void) => {
        if (event === 'publication') {
          publicationCallback = cb;
        }
        return mockSubscription;
      }),
      subscribe: vi.fn(),
      unsubscribe: vi.fn(),
      removeAllListeners: vi.fn(),
    } as unknown as Subscription;

    mockClient = {
      getSubscription: vi.fn().mockReturnValue(null),
      newSubscription: vi.fn().mockReturnValue(mockSubscription),
      removeSubscription: vi.fn(),
    } as unknown as Centrifuge;

    vi.mocked(useCentrifugoContext).mockReturnValue({
      client: mockClient,
      connectionState: 'connected',
    });
  });

  it('returns empty array initially', () => {
    const { result } = renderHook(() => useSystemEvents());
    expect(result.current).toEqual([]);
  });

  it('subscribes to system:events channel on mount', () => {
    renderHook(() => useSystemEvents());
    expect(mockClient.newSubscription).toHaveBeenCalledWith('system:events');
    expect(mockSubscription.on).toHaveBeenCalledWith('publication', expect.any(Function));
    expect(mockSubscription.subscribe).toHaveBeenCalled();
  });

  it('updates events when a publication is received', () => {
    const { result } = renderHook(() => useSystemEvents());

    const event: SystemEvent = {
      type: 'test-event',
      payload: { foo: 'bar' },
      timestamp: new Date().toISOString(),
    };

    act(() => {
      if (publicationCallback) {
        publicationCallback({ data: event } as PublicationContext);
      }
    });

    expect(result.current).toEqual([event]);
  });

  it('unsubscribes and cleans up on unmount', () => {
    const { unmount } = renderHook(() => useSystemEvents());
    unmount();

    expect(mockSubscription.unsubscribe).toHaveBeenCalled();
    expect(mockSubscription.removeAllListeners).toHaveBeenCalled();
    expect(mockClient.removeSubscription).toHaveBeenCalledWith(mockSubscription);
  });

  it('handles existing subscription by cleaning it up first', () => {
    const existingSub = {
      unsubscribe: vi.fn(),
      removeAllListeners: vi.fn(),
    } as unknown as Subscription;
    vi.mocked(mockClient.getSubscription).mockReturnValue(existingSub);

    renderHook(() => useSystemEvents());

    expect(existingSub.unsubscribe).toHaveBeenCalled();
    expect(existingSub.removeAllListeners).toHaveBeenCalled();
    expect(mockClient.removeSubscription).toHaveBeenCalledWith(existingSub);
    expect(mockClient.newSubscription).toHaveBeenCalledWith('system:events');
  });

  it('does nothing if client is not available', () => {
    vi.mocked(useCentrifugoContext).mockReturnValue({
      client: null,
      connectionState: 'disconnected',
    });

    const { result } = renderHook(() => useSystemEvents());
    expect(result.current).toEqual([]);
    expect(mockClient.newSubscription).not.toHaveBeenCalled();
  });
});
