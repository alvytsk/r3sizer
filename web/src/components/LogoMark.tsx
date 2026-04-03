/** Inline crosshair logo mark derived from favicon.svg (no background rect). */
export function LogoMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <rect x="6" y="6" width="20" height="20" rx="2" stroke="currentColor" strokeWidth="1.5" opacity="0.45" />
      <rect x="10" y="10" width="12" height="12" rx="1" stroke="currentColor" strokeWidth="1.5" />
      <line x1="16" y1="4" x2="16" y2="10" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="16" y1="22" x2="16" y2="28" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="4" y1="16" x2="10" y2="16" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <line x1="22" y1="16" x2="28" y2="16" stroke="currentColor" strokeWidth="1" opacity="0.35" />
      <circle cx="16" cy="16" r="1.5" fill="currentColor" />
    </svg>
  );
}
