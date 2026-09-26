export function formatShortcutForDisplay(shortcut: string, platform: string | null) {
  return shortcut
    .split("+")
    .map((part) => {
      if (part === "CmdOrCtrl") {
        if (platform === "macos") {
          return "\u2318";
        }
        if (platform === "windows") {
          return "Ctrl";
        }
      }

      if (platform === "macos") {
        switch (part) {
          case "Shift":
            return "\u21E7";
          case "Alt":
            return "\u2325";
          case "Ctrl":
            return "\u2303";
          default:
            return part;
        }
      }

      return part;
    })
    .join("+");
}

export function formatPasteShortcutForDisplay(platform: string | null) {
  return platform === "macos" ? "\u2318+V" : "Ctrl+V";
}

/** Whole-second durations: "45s", "8m 12s", "1h 5m 3s". */
export function formatDuration(seconds: number) {
  const total = Math.max(0, Math.round(seconds));
  if (total < 60) {
    return `${total}s`;
  }
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainder = total % 60;
  const parts = hours > 0 ? [`${hours}h`, `${minutes}m`] : [`${minutes}m`];
  if (remainder > 0) parts.push(`${remainder}s`);
  return parts.join(" ");
}

export function formatDurationMs(ms: number) {
  return formatDuration(ms / 1000);
}

/** Playback/segment clock: "08:12" below an hour, "1:15:03" from an hour on. */
export function formatClock(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const tail = `${String(Math.floor(seconds / 60) % 60).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`;
  return seconds >= 3600 ? `${Math.floor(seconds / 3600)}:${tail}` : tail;
}

/** Decimal units, as in Finder: 4,221,849 bytes → "4.2 MB". */
export function formatBytes(bytes: number) {
  const value = Math.max(0, bytes);
  if (value < 1000) return `${Math.round(value)} B`;
  if (value < 1_000_000) return `${Math.round(value / 1000)} KB`;
  if (value < 1_000_000_000) {
    const megabytes = value / 1_000_000;
    return megabytes < 10 ? `${megabytes.toFixed(1)} MB` : `${Math.round(megabytes)} MB`;
  }
  return `${(value / 1_000_000_000).toFixed(1)} GB`;
}

// Dates follow German conventions regardless of the system language.
const DATE_LOCALE = "de-DE";

/** "25. Sept." */
export function formatShortDate(iso: string) {
  return new Date(iso).toLocaleDateString(DATE_LOCALE, { day: "numeric", month: "short" });
}

/** "25.09.2026, 23:04" */
export function formatDateTime(iso: string) {
  return new Date(iso).toLocaleString(DATE_LOCALE, { dateStyle: "medium", timeStyle: "short" });
}

/** "23:04" */
export function formatTime(iso: string) {
  return new Date(iso).toLocaleTimeString(DATE_LOCALE, { hour: "2-digit", minute: "2-digit" });
}

/** Clock-style duration for lists: "0:32", "8:13", "1:15:03". */
export function formatListDuration(ms: number) {
  const seconds = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = String(seconds % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Drops an internal error code prefix such as "mic_denied: " so people see the sentence only. */
export function readableError(message: string) {
  return message.replace(/^[a-z][a-z0-9_]*:\s+/, "");
}
