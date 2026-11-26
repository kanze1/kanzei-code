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

export function IconClose({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" className={className} aria-hidden="true">
      <path d="M4 4l8 8M12 4l-8 8" />
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

export function IconSun({ size = 15, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" className={className} aria-hidden="true">
      <circle cx="8" cy="8" r="2.8" />
      <path d="M8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.6 3.6l1 1M11.4 11.4l1 1M12.4 3.6l-1 1M4.6 11.4l-1 1" />
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

export function IconChevronRight({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M6 3.5 10.5 8 6 12.5" />
    </svg>
  );
}

export function IconBranch({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <circle cx="5" cy="3.5" r="1.5" />
      <circle cx="5" cy="12.5" r="1.5" />
      <circle cx="11.5" cy="6.5" r="1.5" />
      <path d="M5 5v6M11.5 8c0 2-2 2.5-6.5 2.5" />
    </svg>
  );
}

export function IconAttach({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M12.2 7.4 7.1 12.5a3 3 0 0 1-4.2-4.2l5.6-5.6a2 2 0 1 1 2.8 2.8L6 10.8a1 1 0 0 1-1.4-1.4l4.7-4.7" />
    </svg>
  );
}

export function IconSend({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8 13V3.5M4 7.2 8 3.2l4 4" />
    </svg>
  );
}

export function IconStop({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} aria-hidden="true">
      <rect x="3.5" y="3.5" width="9" height="9" rx="1.8" />
    </svg>
  );
}

export function IconCopy({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.4" />
      <path d="M10.5 5.5V3.9A1.4 1.4 0 0 0 9.1 2.5H3.9A1.4 1.4 0 0 0 2.5 3.9v5.2a1.4 1.4 0 0 0 1.4 1.4h1.6" />
    </svg>
  );
}

export function IconRefresh({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M13 8a5 5 0 1 1-1.3-3.4" />
      <path d="M13 2.8v2.7h-2.7" />
    </svg>
  );
}

export function IconExternal({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M9.5 2.5h4v4M13.5 2.5 7.5 8.5M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3" />
    </svg>
  );
}

export function IconTarget({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <circle cx="8" cy="8" r="5.5" />
      <circle cx="8" cy="8" r="2.6" />
      <circle cx="8" cy="8" r="0.8" fill="currentColor" />
    </svg>
  );
}

export function IconShield({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8 2 3 4v3.8c0 3 2.1 5.3 5 6.2 2.9-.9 5-3.2 5-6.2V4L8 2Z" />
    </svg>
  );
}

export function IconSparkles({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8 2.2 9.3 6.2 13.3 7.5 9.3 8.8 8 12.8 6.7 8.8 2.7 7.5 6.7 6.2 8 2.2Z" />
    </svg>
  );
}

export function IconStar({ size = 11, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" className={className} aria-hidden="true">
      <path d="M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6 4.2 13.6l.7-4.3-3.1-3 4.3-.6L8 1.8Z" />
    </svg>
  );
}

export function IconTerminal({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <rect x="2" y="3" width="12" height="10" rx="1.8" />
      <path d="M4.8 6.5 7 8.2 4.8 10M8.2 10h2.8" />
    </svg>
  );
}

export function IconDownload({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M8 2.5v7.5M5 7.5 8 10.5l3-3M3 12.8h10" />
    </svg>
  );
}

export function IconCompact({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M3 4.5h10M3 11.5h10M8 2v4M6.2 4.2 8 6l1.8-1.8M8 14v-4M6.2 11.8 8 10l1.8 1.8" />
    </svg>
  );
}

export function IconTrash({ size = 13, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M3 4.5h10M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M4.5 4.5l.7 8.4a1 1 0 0 0 1 .9h3.6a1 1 0 0 0 1-.9l.7-8.4" />
    </svg>
  );
}

export function IconCpu({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <rect x="4.5" y="4.5" width="7" height="7" rx="1.2" />
      <path d="M6.5 2v2.5M9.5 2v2.5M6.5 11.5V14M9.5 11.5V14M2 6.5h2.5M2 9.5h2.5M11.5 6.5H14M11.5 9.5H14" />
    </svg>
  );
}

export function IconLayers({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round" className={className} aria-hidden="true">
      <polygon points="8 2 14 5.2 8 8.4 2 5.2 8 2" />
      <polyline points="2 8.2 8 11.4 14 8.2" />
      <polyline points="2 11.2 8 14.4 14 11.2" />
    </svg>
  );
}

export function IconGlobe({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" className={className} aria-hidden="true">
      <circle cx="8" cy="8" r="5.8" />
      <ellipse cx="8" cy="8" rx="2.6" ry="5.8" />
      <line x1="2.2" y1="8" x2="13.8" y2="8" />
    </svg>
  );
}

export function IconSliders({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" className={className} aria-hidden="true">
      <line x1="2.5" y1="4.5" x2="13.5" y2="4.5" />
      <circle cx="5.5" cy="4.5" r="1.4" fill="var(--bg)" />
      <line x1="2.5" y1="8" x2="13.5" y2="8" />
      <circle cx="10.5" cy="8" r="1.4" fill="var(--bg)" />
      <line x1="2.5" y1="11.5" x2="13.5" y2="11.5" />
      <circle cx="7" cy="11.5" r="1.4" fill="var(--bg)" />
    </svg>
  );
}

export function IconBot({ size = 14, className }: IconProps): React.ReactElement {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <rect x="2.5" y="4.5" width="11" height="8.5" rx="2" />
      <path d="M8 2v2.5M6 8.5h.01M10 8.5h.01M6 10.8h4" />
    </svg>
  );
}
