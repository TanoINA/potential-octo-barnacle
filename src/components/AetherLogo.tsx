import React from 'react';

interface AetherLogoProps {
  className?: string;
  size?: number;
}

export const AetherLogo: React.FC<AetherLogoProps> = ({ className = 'text-[#58a6ff]', size = 16 }) => {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      {/* Outer minimalist diamond */}
      <path
        d="M12 2.5L21.5 12L12 21.5L2.5 12L12 2.5Z"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {/* Inner orbit ring */}
      <circle
        cx="12"
        cy="12"
        r="4"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeOpacity="0.75"
      />
      {/* Core focus point */}
      <circle cx="12" cy="12" r="1.5" fill="currentColor" />
    </svg>
  );
};
