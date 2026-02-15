import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import StatCardTest from './StatCardTest.svelte';

describe('StatCard', () => {
  it('renders value', () => {
    render(StatCardTest, { props: { value: '42', label: 'Total Tracks' } });
    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('renders label', () => {
    render(StatCardTest, { props: { value: '100', label: 'Albums' } });
    expect(screen.getByText('Albums')).toBeInTheDocument();
  });

  it('renders numeric value', () => {
    render(StatCardTest, { props: { value: 999, label: 'Plays' } });
    expect(screen.getByText('999')).toBeInTheDocument();
  });

  it('renders icon snippet', () => {
    render(StatCardTest, { props: { value: '5', label: 'Count' } });
    expect(screen.getByTestId('stat-icon')).toBeInTheDocument();
    expect(screen.getByText('📊')).toBeInTheDocument();
  });

  it('renders card container with correct structure', () => {
    const { container } = render(StatCardTest, { props: { value: '10', label: 'Items' } });
    const card = container.querySelector('.rounded-xl');
    expect(card).toBeInTheDocument();
  });

  it('renders value with bold font', () => {
    const { container } = render(StatCardTest, { props: { value: '25', label: 'Test' } });
    const valueParagraph = container.querySelector('.font-bold');
    expect(valueParagraph).toBeInTheDocument();
    expect(valueParagraph?.textContent).toBe('25');
  });
});
