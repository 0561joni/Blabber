import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatClock,
  formatDateTime,
  formatDuration,
  formatDurationMs,
  formatShortDate,
} from "./formatting";

describe("shared output formatting", () => {
  it("never shows fractional seconds in durations", () => {
    expect(formatDurationMs(60_009)).toBe("1m");
    expect(formatDurationMs(492_629)).toBe("8m 13s");
    expect(formatDurationMs(20_000)).toBe("20s");
    expect(formatDuration(3_723)).toBe("1h 2m 3s");
  });

  it("uses one clock style below and above an hour", () => {
    expect(formatClock(65_999)).toBe("01:05");
    expect(formatClock(4_503_000)).toBe("1:15:03");
  });

  it("uses decimal units like Finder", () => {
    expect(formatBytes(4_221_849)).toBe("4.2 MB");
    expect(formatBytes(487_601_967)).toBe("488 MB");
    expect(formatBytes(4_703_041_355)).toBe("4.7 GB");
    expect(formatBytes(512)).toBe("512 B");
  });

  it("formats dates in German regardless of the system language", () => {
    const iso = new Date(2026, 8, 25, 23, 4).toISOString();
    expect(formatShortDate(iso)).toMatch(/^25\. Sept?\.$/);
    expect(formatDateTime(iso)).toBe("25.09.2026, 23:04");
  });
});
