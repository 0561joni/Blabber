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
  | "window";

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
  switch (name) {
    case "home":
      return <><path d="M4 11.5 12 5l8 6.5" /><path d="M6.5 10.5V19h11v-8.5" /></>;
    case "book":
      return <><path d="M6 5.5A2.5 2.5 0 0 1 8.5 3H18v16H8.5A2.5 2.5 0 0 0 6 21.5Z" /><path d="M6 5.5v16" /><path d="M9.5 7.5H15M9.5 11H15" /></>;
    case "gear":
      return <><path d="m12 4 1.2 2.2 2.5.5.7 2.4 2 1.6-.8 2.4.8 2.4-2 1.6-.7 2.4-2.5.5L12 20l-1.2-2.2-2.5-.5-.7-2.4-2-1.6.8-2.4-.8-2.4 2-1.6.7-2.4 2.5-.5Z" /><circle cx="12" cy="12" r="3.1" /></>;
    case "clock":
      return <><circle cx="12" cy="12" r="8.5" /><path d="M12 7.8v4.6l3 1.8" /></>;
    case "chevronLeft":
      return <path d="m14.5 6-6 6 6 6" />;
    case "chevronRight":
      return <path d="m9.5 6 6 6-6 6" />;
    case "trash":
      return <><path d="M4 7h16M9 4h6l1 3M6.5 7l.7 13h9.6l.7-13" /><path d="M10 11v5M14 11v5" /></>;
    case "trashMultiple":
      return <><path d="M3.5 8h13M7 5h6l1 3M5.5 8l.6 12h7.8l.6-12M9 11v5M12 11v5" /><path d="M16 5h3v13h-2M14 2h7v13" /></>;
    case "pencil":
      return <><path d="m4 20 4.3-1 10.9-10.9a2.2 2.2 0 0 0-3.1-3.1L5.2 15.9Z" /><path d="m14.7 6.4 3 3M5.2 15.9l3.1 3.1" /></>;
    case "copy":
      return <><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></>;
    case "copyPlain":
      return <><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2M11 12h6M11 15h6M11 18h4" /></>;
    case "copySpeakers":
      return <><path d="M15.5 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7.5a2 2 0 0 0 2 2h2" /><rect x="8" y="8" width="12" height="12" rx="2" /><circle cx="13" cy="12" r="1.5" /><circle cx="17" cy="13" r="1.2" /><path d="M10.7 17c.4-1.5 1.2-2.3 2.3-2.3s2 .8 2.4 2.3M15.5 16c.4-.9.9-1.4 1.6-1.4.8 0 1.4.6 1.7 1.7" /></>;
    case "share":
      return <><path d="M12 15V3M8 7l4-4 4 4" /><path d="M6 10H5a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7a2 2 0 0 0-2-2h-1" /></>;
    case "retry":
      return <><path d="M19.5 8.5V4.8l-3.7.1" /><path d="M19 9a8 8 0 1 0 .7 5" /></>;
    case "retrySpeakers":
      return <><path d="M19.5 8V4.5L16 5M19 8.5A8 8 0 0 0 5.2 6" /><circle cx="10" cy="12" r="2" /><circle cx="15" cy="13" r="1.5" /><path d="M6.8 18c.5-2.1 1.6-3.2 3.2-3.2s2.8 1.1 3.3 3.2M13.5 17.5c.4-1.5 1-2.2 2-2.2 1.1 0 1.9.8 2.3 2.3" /></>;
    case "check":
      return <path d="m5 12.5 4.3 4.3L19.5 6.5" />;
    case "xmark":
      return <path d="m6 6 12 12M18 6 6 18" />;
    case "xCircle":
      return <><circle cx="12" cy="12" r="9" /><path d="m8.5 8.5 7 7M15.5 8.5l-7 7" /></>;
    case "plus":
      return <path d="M12 5v14M5 12h14" />;
    case "folder":
      return <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H10l2 2h6.5A2.5 2.5 0 0 1 21 9.5v7A2.5 2.5 0 0 1 18.5 19h-13A2.5 2.5 0 0 1 3 16.5Z" />;
    case "download":
      return <><path d="M12 3v11M8 10l4 4 4-4" /><path d="M4 16v3h16v-3" /></>;
    case "keyboardEdit":
      return <><rect x="2.5" y="5" width="16" height="12" rx="2" /><path d="M5.5 9h1M9 9h1M12.5 9h1M5.5 12h1M9 12h1M6 15h6" /><path d="m14.5 18.8.7-2.7 4.9-4.9a1.3 1.3 0 0 1 1.8 1.8L17 17.9Z" /></>;
    case "reset":
      return <><path d="M4.5 8.5V4.8l3.7.1" /><path d="M5 9a8 8 0 1 1-.7 5" /></>;
    case "microphone":
      return <><rect x="9" y="3" width="6" height="12" rx="3" /><path d="M6 11a6 6 0 0 0 12 0M12 17v4M9 21h6" /></>;
    case "microphoneActive":
      return <><rect x="9" y="3" width="6" height="12" rx="3" /><path d="M6 11a6 6 0 0 0 12 0M12 17v4M9 21h6M2.5 9v3M21.5 8v5" /></>;
    case "stop":
      return <rect x="6" y="6" width="12" height="12" rx="2.5" />;
    case "upload":
      return <><path d="M12 15V3M8 7l4-4 4 4" /><path d="M4 16v4h16v-4" /></>;
    case "personAutomatic":
      return <><circle cx="9" cy="9" r="2.5" /><circle cx="16" cy="10" r="2" /><path d="M4.5 18c.6-3 2.1-4.5 4.5-4.5s4 1.5 4.6 4.5M14 14.5c1.8-.7 3.7.2 4.5 2.5" /><path d="m18.5 3 .5 1.3 1.3.5-1.3.5-.5 1.3-.5-1.3-1.3-.5 1.3-.5Z" /></>;
    case "personCount":
      return <><circle cx="9" cy="9" r="2.5" /><circle cx="16" cy="10" r="2" /><path d="M4.5 18c.6-3 2.1-4.5 4.5-4.5s4 1.5 4.6 4.5M14 14.5c1.8-.7 3.7.2 4.5 2.5" /></>;
    case "disclosure":
      return <path d="m9 6 7 6-7 6Z" fill="currentColor" stroke="none" />;
    case "power":
      return <><path d="M12 3v8" /><path d="M7.2 6.2a8 8 0 1 0 9.6 0" /></>;
    case "window":
      return <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 8h18M7 6h.1M10 6h.1" /></>;
  }
}
