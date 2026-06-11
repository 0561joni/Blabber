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

export function formatDuration(seconds: number) {
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder === 0 ? `${minutes}m` : `${minutes}m ${remainder}s`;
}
