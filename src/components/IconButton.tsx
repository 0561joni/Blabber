import { forwardRef, useId, type ButtonHTMLAttributes } from "react";

export type AppIconName =
  | "home"
  | "book"
  | "gear"
  | "clock"
  | "chevronLeft"
  | "chevronRight"
  | "trash"
  | "trashMultiple"
  | "pencil"
  | "copy"
  | "copyPlain"
  | "copySpeakers"
  | "share"
  | "retry"
  | "retrySpeakers"
  | "check"
  | "xmark"
  | "xCircle"
  | "plus"
  | "folder"
  | "download"
  | "info"
  | "keyboardEdit"
  | "reset"
  | "microphone"
  | "microphoneActive"
  | "stop"
  | "upload"
  | "personAutomatic"
  | "personCount"
  | "disclosure"
  | "power"
  | "window"
  | "library"
  | "fileAudio"
  | "search"
  | "play"
  | "pause"
  | "more"
  | "languages";

interface AppIconProps {
  name: AppIconName;
  badge?: number | string;
  className?: string;
}

export function AppIcon({ name, badge, className }: AppIconProps) {
  const paths = iconPaths(name);
  return (
    <span className={["app-icon-wrap", className].filter(Boolean).join(" ")} aria-hidden="true">
      <svg className="app-icon" viewBox="0 0 24 24" fill="none">
        {paths}
      </svg>
      {badge !== undefined ? <span className="app-icon-badge">{badge}</span> : null}
    </span>
  );
}

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: AppIconName;
  label: string;
  size?: "standard" | "compact";
  tone?: "default" | "danger";
  state?: "default" | "selected" | "busy" | "success" | "error";
  badge?: number | string;
  tooltipPlacement?: "top" | "bottom";
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    icon,
    label,
    size = "standard",
    tone = "default",
    state = "default",
    badge,
    tooltipPlacement = "top",
    className,
    type = "button",
    ...buttonProps
  },
  ref,
) {
  const tooltipId = useId();
  return (
    <button
      {...buttonProps}
      ref={ref}
      type={type}
      aria-label={buttonProps["aria-label"] ?? label}
      aria-describedby={buttonProps["aria-describedby"] ?? tooltipId}
      className={[
        "icon-button",
        `icon-button--${size}`,
        tone === "danger" ? "icon-button--danger" : "",
        state !== "default" ? `icon-button--${state}` : "",
        className,
      ]
        .filter(Boolean)
        .join(" ")}
    >
      {state === "busy" ? (
        <span className="icon-button-spinner" aria-hidden="true" />
      ) : (
        <AppIcon name={icon} badge={badge} />
      )}
      <span id={tooltipId} className={`icon-button-tooltip tooltip-${tooltipPlacement}`} role="tooltip">
        {label}
      </span>
    </button>
  );
});

function iconPaths(name: AppIconName) {
  // Lucide icons (ISC license, lucide.dev), inlined so the app needs no network or extra package.
  switch (name) {
    case "home":
      return <><path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" /><path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></>;
    case "book":
      return <><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20" /><path d="m8 13 4-7 4 7" /><path d="M9.1 11h5.7" /></>;
    case "gear":
      return <><path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" /><circle cx="12" cy="12" r="3" /></>;
    case "clock":
      return <><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></>;
    case "chevronLeft":
      return <><path d="m15 18-6-6 6-6" /></>;
    case "chevronRight":
      return <><path d="m9 18 6-6-6-6" /></>;
    case "trash":
      return <><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></>;
    case "trashMultiple":
      return <><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></>;
    case "pencil":
      return <><path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" /><path d="m15 5 4 4" /></>;
    case "copy":
      return <><rect width="14" height="14" x="8" y="8" rx="2" ry="2" /><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" /></>;
    case "copyPlain":
      return <><path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" /><path d="M14 2v5a1 1 0 0 0 1 1h5" /><path d="M10 9H8" /><path d="M16 13H8" /><path d="M16 17H8" /></>;
    case "copySpeakers":
      return <><path d="M17 5H3" /><path d="M21 12H8" /><path d="M21 19H8" /><path d="M3 12v7" /></>;
    case "share":
      return <><path d="M12 2v13" /><path d="m16 6-4-4-4 4" /><path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" /></>;
    case "retry":
      return <><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /></>;
    case "retrySpeakers":
      return <><circle cx="10" cy="8" r="5" /><path d="M2 21a8 8 0 0 1 10.434-7.62" /><circle cx="18" cy="18" r="3" /><path d="m22 22-1.9-1.9" /></>;
    case "check":
      return <><path d="M20 6 9 17l-5-5" /></>;
    case "xmark":
      return <><path d="M18 6 6 18" /><path d="m6 6 12 12" /></>;
    case "xCircle":
      return <><circle cx="12" cy="12" r="10" /><path d="m15 9-6 6" /><path d="m9 9 6 6" /></>;
    case "plus":
      return <><path d="M5 12h14" /><path d="M12 5v14" /></>;
    case "folder":
      return <><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" /></>;
    case "download":
      return <><path d="M12 15V3" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" /></>;
    case "info":
      return <><circle cx="12" cy="12" r="10" /><path d="M12 16v-4" /><path d="M12 8h.01" /></>;
    case "keyboardEdit":
      return <><path d="M10 8h.01" /><path d="M12 12h.01" /><path d="M14 8h.01" /><path d="M16 12h.01" /><path d="M18 8h.01" /><path d="M6 8h.01" /><path d="M7 16h10" /><path d="M8 12h.01" /><rect width="20" height="16" x="2" y="4" rx="2" /></>;
    case "reset":
      return <><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /></>;
    case "microphone":
      return <><path d="M12 19v3" /><path d="M19 10v2a7 7 0 0 1-14 0v-2" /><rect x="9" y="2" width="6" height="13" rx="3" /></>;
    case "microphoneActive":
      return <><path d="M2 10v3" /><path d="M6 6v11" /><path d="M10 3v18" /><path d="M14 8v7" /><path d="M18 5v13" /><path d="M22 10v3" /></>;
    case "stop":
      return <><rect width="18" height="18" x="3" y="3" rx="2" /></>;
    case "upload":
      return <><path d="M12 3v12" /><path d="m17 8-5-5-5 5" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /></>;
    case "personAutomatic":
      return <><path d="m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72" /><path d="m14 7 3 3" /><path d="M5 6v4" /><path d="M19 14v4" /><path d="M10 2v2" /><path d="M7 8H3" /><path d="M21 16h-4" /><path d="M11 3H9" /></>;
    case "personCount":
      return <><path d="M18 21a8 8 0 0 0-16 0" /><circle cx="10" cy="8" r="5" /><path d="M22 20c0-3.37-2-6.5-4-8a5 5 0 0 0-.45-8.3" /></>;
    case "disclosure":
      return <><path d="m9 18 6-6-6-6" /></>;
    case "power":
      return <><path d="M12 2v10" /><path d="M18.4 6.6a9 9 0 1 1-12.77.04" /></>;
    case "window":
      return <><rect x="2" y="4" width="20" height="16" rx="2" /><path d="M10 4v4" /><path d="M2 8h20" /><path d="M6 4v4" /></>;
    case "library":
      return <><path d="m16 6 4 14" /><path d="M12 6v14" /><path d="M8 8v12" /><path d="M4 4v16" /></>;
    case "fileAudio":
      return <><path d="M4 6.835V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.706.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2h-.343" /><path d="M14 2v5a1 1 0 0 0 1 1h5" /><path d="M2 19a2 2 0 0 1 4 0v1a2 2 0 0 1-4 0v-4a6 6 0 0 1 12 0v4a2 2 0 0 1-4 0v-1a2 2 0 0 1 4 0" /></>;
    case "search":
      return <><path d="m21 21-4.34-4.34" /><circle cx="11" cy="11" r="8" /></>;
    case "play":
      return <><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z" /></>;
    case "pause":
      return <><rect x="14" y="3" width="5" height="18" rx="1" /><rect x="5" y="3" width="5" height="18" rx="1" /></>;
    case "more":
      return <><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /><circle cx="5" cy="12" r="1" /></>;
    case "languages":
      return <><path d="m5 8 6 6" /><path d="m4 14 6-6 2-3" /><path d="M2 5h12" /><path d="M7 2h1" /><path d="m22 22-5-10-5 10" /><path d="M14 18h6" /></>;
  }
}
