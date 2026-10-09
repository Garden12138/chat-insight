import { useId } from "react";

export function BrandMark({ size = 34, className = "" }: { size?: number; className?: string }) {
  const gradient = useId();
  return (
    <svg className={`brand-mark ${className}`} width={size} height={size} viewBox="0 0 96 96" aria-hidden="true">
      <defs>
        <linearGradient id={gradient} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ff9a76" />
          <stop offset="1" stopColor="#f1442a" />
        </linearGradient>
      </defs>
      <rect x="4" y="4" width="88" height="88" rx="24" fill={`url(#${gradient})`} />
      <path d="M22 32a9 9 0 0 1 9-9h34a9 9 0 0 1 9 9v22a9 9 0 0 1-9 9H46L33 74l3-11h-5a9 9 0 0 1-9-9z" fill="#fff6ee" />
      <circle cx="37" cy="40" r="5" fill="#f1442a" />
      <path d="M28 56l9-8 7 5 9-11 12 14z" fill="#ffc2a8" />
    </svg>
  );
}
