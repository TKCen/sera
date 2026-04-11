import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatCard } from './StatCard';

describe('StatCard', () => {
  it('renders label and value', () => {
    render(<StatCard label="Test Label" value="1,234" />);
    expect(screen.getByText('Test Label')).toBeDefined();
    expect(screen.getByText('1,234')).toBeDefined();
  });

  it('renders positive trend correctly', () => {
    render(<StatCard label="Test" value="100" trend={15} />);
    const trendContainer = screen.getByText('+15%').parentElement;
    expect(trendContainer?.className).toContain('text-sera-success');
    expect(screen.getByText('+15%')).toBeDefined();
  });

  it('renders negative trend correctly', () => {
    render(<StatCard label="Test" value="100" trend={-5} />);
    const trendContainer = screen.getByText('-5%').parentElement;
    expect(trendContainer?.className).toContain('text-sera-error');
    expect(screen.getByText('-5%')).toBeDefined();
  });

  it('renders trend label if provided', () => {
    render(<StatCard label="Test" value="100" trend={10} trendLabel="vs last week" />);
    expect(screen.getByText('+10% vs last week')).toBeDefined();
  });
});
