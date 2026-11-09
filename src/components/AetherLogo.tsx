import React from 'react';

interface AetherLogoProps {
  className?: string;
  size?: number;
}

export const AetherLogo: React.FC<AetherLogoProps> = ({ className = 'text-[#a1a1aa]', size = 16 }) => {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      role="img"
      aria-label="Aether"
      focusable="false"
    >
      <path
        d="M5 20L12 4L19 20M8 14H16"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
};
