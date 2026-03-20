import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { BudgetBar } from '@/components/BudgetBar';
import { budgetBarColor } from '@/lib/utils';

describe('budgetBarColor', () => {
  it('returns success colour below 70%', () => {
    expect(budgetBarColor(0)).toBe('bg-sera-success');
    expect(budgetBarColor(50)).toBe('bg-sera-success');
    expect(budgetBarColor(69)).toBe('bg-sera-success');
  });

  it('returns warning colour at 70–89%', () => {
    expect(budgetBarColor(70)).toBe('bg-sera-warning');
    expect(budgetBarColor(80)).toBe('bg-sera-warning');
    expect(budgetBarColor(89)).toBe('bg-sera-warning');
  });

  it('returns error colour at 90% and above', () => {
    expect(budgetBarColor(90)).toBe('bg-sera-error');
    expect(budgetBarColor(95)).toBe('bg-sera-error');
    expect(budgetBarColor(100)).toBe('bg-sera-error');
  });
});

describe('BudgetBar', () => {
  it('renders green bar below 70%', () => {
    render(<BudgetBar label="Hourly" current={500} limit={1000} />);
    const fill = screen.getByTestId('budget-bar-fill');
    expect(fill.className).toContain('bg-sera-success');
    expect(fill.getAttribute('data-pct')).toBe('50');
  });

  it('renders amber bar at 80%', () => {
    render(<BudgetBar label="Hourly" current={800} limit={1000} />);
    const fill = screen.getByTestId('budget-bar-fill');
    expect(fill.className).toContain('bg-sera-warning');
    expect(fill.getAttribute('data-pct')).toBe('80');
  });

  it('renders red bar at 95%', () => {
    render(<BudgetBar label="Hourly" current={950} limit={1000} />);
    const fill = screen.getByTestId('budget-bar-fill');
    expect(fill.className).toContain('bg-sera-error');
    expect(fill.getAttribute('data-pct')).toBe('95');
  });

  it('renders 0% bar when no limit set', () => {
    render(<BudgetBar label="Hourly" current={500} />);
    const fill = screen.getByTestId('budget-bar-fill');
    expect(fill).toHaveStyle({ width: '0%' });
  });

  it('caps bar at 100% even if usage exceeds limit', () => {
    render(<BudgetBar label="Hourly" current={1500} limit={1000} />);
    const fill = screen.getByTestId('budget-bar-fill');
    expect(fill.getAttribute('data-pct')).toBe('100');
  });

  it('shows warning text at 70%', () => {
    render(<BudgetBar label="Hourly" current={700} limit={1000} />);
    expect(screen.getByText(/Approaching limit/)).toBeInTheDocument();
  });

  it('shows exceeded text at 100%', () => {
    render(<BudgetBar label="Hourly" current={1000} limit={1000} />);
    expect(screen.getByText(/Budget exceeded/)).toBeInTheDocument();
  });

  it('shows no warning text below 70%', () => {
    render(<BudgetBar label="Hourly" current={600} limit={1000} />);
    expect(screen.queryByText(/limit|exceeded/i)).not.toBeInTheDocument();
  });
});
