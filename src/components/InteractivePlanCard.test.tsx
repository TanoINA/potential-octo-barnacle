import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { InteractivePlanCard } from './InteractivePlanCard';

describe('InteractivePlanCard', () => {
  const samplePlan = `
- [x] Step 1: Analyze directory layout
- [-] Step 2: Implement core algorithm
- [ ] Step 3: Run regression tests
`;

  it('renders plan items with completion counts and progress percent', () => {
    render(<InteractivePlanCard rawPlan={samplePlan} />);
    expect(screen.getByText('Execution Plan')).toBeTruthy();
    expect(screen.getByText('1/3 completed')).toBeTruthy();
    expect(screen.getByText('33%')).toBeTruthy();
    expect(screen.getByText('Step 1: Analyze directory layout')).toBeTruthy();
    expect(screen.getByText('Step 2: Implement core algorithm')).toBeTruthy();
    expect(screen.getByText('Step 3: Run regression tests')).toBeTruthy();
  });

  it('allows toggling plan item status on click', () => {
    render(<InteractivePlanCard rawPlan={samplePlan} />);
    const pendingStep = screen.getByText('Step 3: Run regression tests');

    fireEvent.click(pendingStep);
    // After clicking pending item, completed count becomes 2/3 (67%)
    expect(screen.getByText('2/3 completed')).toBeTruthy();
    expect(screen.getByText('67%')).toBeTruthy();

    fireEvent.click(pendingStep);
    // After clicking again, reverts to 1/3
    expect(screen.getByText('1/3 completed')).toBeTruthy();
  });
});
