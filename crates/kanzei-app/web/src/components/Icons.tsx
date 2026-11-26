import React from "react";

interface IconProps {
  size?: number;
  className?: string;
}

export function IconSidebar({ size = 16, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" className={className} aria-hidden="true">
      <rect x="2" y="2.5" width="12" height="11" rx="2" />
      <line x1="6" y1="2.5" x2="6" y2="13.5" />
    </svg>
  );
}

export function IconPlus({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" className={className} aria-hidden="true">
      <line x1="8" y1="3" x2="8" y2="13" />
      <line x1="3" y1="8" x2="13" y2="8" />
    </svg>
  );
}

export function IconFolder({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h3l1.5 1.5H12.5A1.5 1.5 0 0 1 14 6v6a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 12V4.5Z" />
    </svg>
  );
}

export function IconChat({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M2.5 3.5h11v7.5H6.5L3.5 13.5V11h-1V3.5Z" />
    </svg>
  );
}

export function IconMemory({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <circle cx="8" cy="4" r="2" />
      <circle cx="3.5" cy="11.5" r="2" />
      <circle cx="12.5" cy="11.5" r="2" />
      <line x1="7" y1="5.7" x2="4.5" y2="9.8" />
      <line x1="9" y1="5.7" x2="11.5" y2="9.8" />
    </svg>
  );
}

export function IconSkills({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8.8 2 3.5 9h4.2l-.5 5L12.5 7H8.3l.5-5Z" />
    </svg>
  );
}

export function IconSettings({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <circle cx="8" cy="8" r="2.3" />
      <path d="M8 1.8v1.6M8 12.6v1.6M1.8 8h1.6M12.6 8h1.6M3.6 3.6l1.1 1.1M11.3 11.3l1.1 1.1M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1" strokeLinecap="round" />
    </svg>
  );
}

export function IconTheme({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <path d="M12.8 9.8A5.2 5.2 0 0 1 6.2 3.2a5.5 5.5 0 1 0 6.6 6.6Z" />
    </svg>
  );
}

export function IconFileCode({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M3.5 2h6l3 3v9h-9V2Z" />
      <path d="M9.5 2v3h3" />
    </svg>
  );
}

export function IconActivity({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <polyline points="1.5 8.5 4.5 8.5 6.5 3.5 9.5 12.5 11.5 8.5 14.5 8.5" />
    </svg>
  );
}

export function IconSearch({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" className={className} aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" />
      <line x1="10.5" y1="10.5" x2="14" y2="14" />
    </svg>
  );
}

export function IconGrid({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <rect x="2.2" y="2.2" width="4.6" height="4.6" rx="1" />
      <rect x="9.2" y="2.2" width="4.6" height="4.6" rx="1" />
      <rect x="2.2" y="9.2" width="4.6" height="4.6" rx="1" />
      <rect x="9.2" y="9.2" width="4.6" height="4.6" rx="1" />
    </svg>
  );
}
