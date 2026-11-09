import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { AetherLogo } from './AetherLogo';

describe('AetherLogo', () => {
  it('renders a labeled, neutral geometric mark without orbit or animation effects', () => {
    const { container, rerender } = render(<AetherLogo />);
    const logo = screen.getByRole('img', { name: 'Aether' });
    expect(logo.getAttribute('class')).toBe('text-[#a1a1aa]');
    expect(logo.getAttribute('viewBox')).toBe('0 0 24 24');
    expect(container.querySelector('path')?.getAttribute('d')).toBe('M5 20L12 4L19 20M8 14H16');
    expect(container.querySelector('circle, animate, animateTransform, filter')).toBeNull();
    expect(container.innerHTML).not.toMatch(/animate-|glow|shimmer|drop-shadow/);

    rerender(<AetherLogo size={32} className="text-current" />);
    expect(screen.getByRole('img', { name: 'Aether' }).getAttribute('width')).toBe('32');
    expect(screen.getByRole('img', { name: 'Aether' }).getAttribute('class')).toBe('text-current');
  });
});
