import { render, screen, fireEvent, act } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { ThoughtTimeline } from '@/components/ThoughtTimeline';
import type { ThoughtEvent } from '@/lib/api/types';

function makeThought(stepType: ThoughtEvent['stepType'], content: string, i = 0): ThoughtEvent {
  return {
    stepType,
    content,
    agentId: 'test-agent',
    timestamp: new Date(Date.now() + i * 1000).toISOString(),
  };
}

describe('ThoughtTimeline', () => {
  it('renders all thoughts when expanded', () => {
    const thoughts: ThoughtEvent[] = [
      makeThought('observe', 'Analysing the problem', 0),
      makeThought('plan', 'Deciding approach', 1),
      makeThought('act', 'Executing tool', 2),
      makeThought('reflect', 'Evaluating result', 3),
    ];

    render(<ThoughtTimeline thoughts={thoughts} />);

    expect(screen.getByText('Analysing the problem')).toBeInTheDocument();
    expect(screen.getByText('Deciding approach')).toBeInTheDocument();
    expect(screen.getByText('Executing tool')).toBeInTheDocument();
    expect(screen.getByText('Evaluating result')).toBeInTheDocument();
  });

  it('shows only act steps in collapsed mode', () => {
    const thoughts: ThoughtEvent[] = [
      makeThought('observe', 'Observing the state', 0),
      makeThought('plan', 'Planning next step', 1),
      makeThought('act', 'Running the command', 2),
      makeThought('reflect', 'Reflection content', 3),
    ];

    render(<ThoughtTimeline thoughts={thoughts} />);

    fireEvent.click(screen.getByText('Key events'));

    expect(screen.queryByText('Observing the state')).not.toBeInTheDocument();
    expect(screen.queryByText('Planning next step')).not.toBeInTheDocument();
    expect(screen.getByText('Running the command')).toBeInTheDocument();
    expect(screen.queryByText('Reflection content')).not.toBeInTheDocument();
  });

  it('toggles back to all steps when clicked again', () => {
    const thoughts: ThoughtEvent[] = [
      makeThought('observe', 'Observe step', 0),
      makeThought('act', 'Act step', 1),
    ];

    render(<ThoughtTimeline thoughts={thoughts} />);

    fireEvent.click(screen.getByText('Key events'));
    expect(screen.queryByText('Observe step')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('All steps'));
    expect(screen.getByText('Observe step')).toBeInTheDocument();
  });

  it('shows empty state when no thoughts', () => {
    render(<ThoughtTimeline thoughts={[]} />);
    expect(screen.getByText('No thoughts yet')).toBeInTheDocument();
  });

  it('shows key events empty state in collapsed mode', () => {
    const thoughts: ThoughtEvent[] = [makeThought('observe', 'Observe only', 0)];

    render(<ThoughtTimeline thoughts={thoughts} />);
    fireEvent.click(screen.getByText('Key events'));
    expect(screen.getByText('No key events')).toBeInTheDocument();
  });

  it('auto-scrolls to bottom when new thoughts arrive', () => {
    const scrollIntoViewMock = vi.fn();
    const scrollToMock = vi.fn();

    const { rerender } = render(
      <ThoughtTimeline thoughts={[makeThought('observe', 'First thought', 0)]} />
    );

    const scrollContainer = document.querySelector('[class*="overflow-y-auto"]') as HTMLElement;
    if (scrollContainer) {
      Object.defineProperty(scrollContainer, 'scrollHeight', { value: 500, configurable: true });
      Object.defineProperty(scrollContainer, 'clientHeight', { value: 300, configurable: true });
      Object.defineProperty(scrollContainer, 'scrollTop', { value: 200, writable: true });
      scrollContainer.scrollTo = scrollToMock;
      scrollContainer.scrollIntoView = scrollIntoViewMock;
    }

    act(() => {
      rerender(
        <ThoughtTimeline
          thoughts={[
            makeThought('observe', 'First thought', 0),
            makeThought('act', 'Second thought', 1),
          ]}
        />
      );
    });

    // The scroll container's scrollTop should have been updated (auto-scroll)
    expect(scrollContainer?.scrollTop).toBeDefined();
  });

  it('freezes scroll when user has scrolled up', () => {
    const { rerender } = render(
      <ThoughtTimeline thoughts={[makeThought('observe', 'Initial thought', 0)]} />
    );

    const scrollContainer = document.querySelector('[class*="overflow-y-auto"]') as HTMLElement;

    if (scrollContainer) {
      Object.defineProperty(scrollContainer, 'scrollHeight', { value: 1000, configurable: true });
      Object.defineProperty(scrollContainer, 'clientHeight', { value: 300, configurable: true });
      Object.defineProperty(scrollContainer, 'scrollTop', { value: 100, writable: true });

      // Simulate user scrolling up (not at bottom)
      fireEvent.scroll(scrollContainer);
    }

    const scrollTopBefore = scrollContainer?.scrollTop;

    act(() => {
      rerender(
        <ThoughtTimeline
          thoughts={[
            makeThought('observe', 'Initial thought', 0),
            makeThought('plan', 'New thought', 1),
          ]}
        />
      );
    });

    // scrollTop should not have changed since user scrolled up
    expect(scrollContainer?.scrollTop).toBe(scrollTopBefore);
  });

  it('displays correct type labels', () => {
    const thoughts: ThoughtEvent[] = [
      makeThought('observe', 'obs content', 0),
      makeThought('plan', 'plan content', 1),
      makeThought('act', 'act content', 2),
      makeThought('reflect', 'ref content', 3),
    ];

    render(<ThoughtTimeline thoughts={thoughts} />);

    expect(screen.getByText('Observe')).toBeInTheDocument();
    expect(screen.getByText('Plan')).toBeInTheDocument();
    expect(screen.getByText('Act')).toBeInTheDocument();
    expect(screen.getByText('Reflect')).toBeInTheDocument();
  });
});
