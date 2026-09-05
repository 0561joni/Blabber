import {
  useEffect,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type ReactNode,
} from "react";
import { AppIcon, type AppIconName } from "./IconButton";

export function Button({
  children,
  icon,
  busy,
  variant = "secondary",
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  icon?: AppIconName;
  busy?: boolean;
  variant?: "primary" | "secondary" | "danger" | "ghost";
}) {
  return (
    <button
      type="button"
      {...props}
      aria-busy={busy || undefined}
      disabled={props.disabled || busy}
      className={["button", "button-" + variant, className].join(" ")}
    >
      {busy ? (
        <span className="icon-button-spinner" aria-hidden="true" />
      ) : icon ? (
        <AppIcon name={icon} />
      ) : null}
      {children}
    </button>
  );
}

export function ActionButton({
  action,
  children,
  success = "Done",
  ...props
}: Omit<Parameters<typeof Button>[0], "onClick"> & {
  action: () => Promise<unknown>;
  success?: string;
}) {
  const [state, setState] = useState<"idle" | "busy" | "success" | "error">(
    "idle",
  );
  const [error, setError] = useState("");
  const inFlight = useRef(false);
  useEffect(() => {
    if (state !== "success") return;
    const timer = window.setTimeout(() => setState("idle"), 2200);
    return () => window.clearTimeout(timer);
  }, [state]);
  return (
    <span className="action-feedback">
      <Button
        {...props}
        busy={state === "busy"}
        icon={state === "success" ? "check" : props.icon}
        onClick={async () => {
          if (inFlight.current) return;
          inFlight.current = true;
          setState("busy");
          try {
            await action();
            setState(success ? "success" : "idle");
          } catch (reason) {
            setError(
              reason instanceof Error ? reason.message : "Please try again.",
            );
            setState("error");
          } finally {
            inFlight.current = false;
          }
        }}
      >
        {state === "success" ? success : children}
      </Button>
      <span
        className={state === "error" ? "error-text action-message" : "sr-only"}
        role={state === "error" ? "alert" : "status"}
      >
        {state === "error" ? error : state === "success" ? success : ""}
      </span>
    </span>
  );
}

export function PageHeader({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  description?: string;
  children?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        {description ? <p className="muted">{description}</p> : null}
      </div>
      {children}
    </header>
  );
}

export function Progress({
  value,
  label,
}: {
  value?: number | null;
  label: string;
}) {
  return (
    <div
      className="progress-track"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={
        value == null
          ? undefined
          : Math.round(Math.max(0, Math.min(100, value)))
      }
    >
      <div
        className={"progress-fill" + (value == null ? " indeterminate" : "")}
        style={
          value == null
            ? undefined
            : { width: Math.max(0, Math.min(100, value)) + "%" }
        }
      />
    </div>
  );
}
