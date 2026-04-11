import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { EmptyState } from './EmptyState';

describe('EmptyState', () => {
  it('renders title and description', () => {
    render(<EmptyState title="No Items" description="Add an item to get started." />);
    expect(screen.getByText('No Items')).toBeDefined();
    expect(screen.getByText('Add an item to get started.')).toBeDefined();
  });

  it('renders icon if provided', () => {
    render(<EmptyState title="No Items" icon={<span data-testid="test-icon">Icon</span>} />);
    expect(screen.getByTestId('test-icon')).toBeDefined();
  });

  it('renders action if provided', () => {
    render(<EmptyState title="No Items" action={<button>Add Item</button>} />);
    expect(screen.getByRole('button', { name: 'Add Item' })).toBeDefined();
  });
});
