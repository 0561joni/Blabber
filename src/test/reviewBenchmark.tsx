// Explicit development fixture, omitted from the production build entry points.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  VirtualPassages,
  type PassageListHandle,
} from "../components/VirtualPassages";
import { speakerMap } from "../lib/speakerLabels";
import { reviewFixture } from "./reviewFixture";
import "../styles.css";
function Benchmark() {
  const data = useMemo(() => {
    const fixture = reviewFixture(10000);
    for (const [i, s] of fixture.detail.segments.entries())
      s.text = s.text.repeat(1 + (i % 8));
    return fixture;
  }, []);
  const names = useMemo(() => speakerMap(data.detail.speakers), [data]);
  const list = useRef<PassageListHandle>(null);
  const [selected, setSelected] = useState(new Set<string>());
  const [filtered, setFiltered] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const [theme, setTheme] = useState("dark");
  const [timings, setTimings] = useState<string[]>([]);
  const manual = useMemo(() => new Set<string>(), []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  const measure = useCallback((label: string, action: () => void) => {
    const start = performance.now();
    action();
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        setTimings((t) => [
          ...t.slice(-9),
          `${label}: ${(performance.now() - start).toFixed(1)} ms`,
        ]),
      ),
    );
  }, []);
  const select = useCallback(
    (id: string) =>
      measure("Select passage", () =>
        setSelected((s) => {
          const n = new Set(s);
          n.has(id) ? n.delete(id) : n.add(id);
          return n;
        }),
      ),
    [measure],
  );
  const assign = useCallback(
    (id: string) =>
      measure("Open passage selection", () => setSelected(new Set([id]))),
    [measure],
  );
  const seek = useCallback(
    (ms: number) =>
      measure("Playback highlight", () =>
        setActive(`passage-${Math.floor(ms / 6000)}`),
      ),
    [measure],
  );
  const noop = useCallback(() => {}, []);
  return (
    <main
      data-theme={theme}
      style={{
        background: "var(--canvas)",
        color: "var(--text-primary)",
        padding: 24,
        minHeight: "100vh",
      }}
    >
      <h1>10,000-passage review fixture</h1>
      <p>
        Local synthetic rendering benchmark. Times include two animation frames
        after each interaction.
      </p>
      <div className="review-tools">
        <button
          onClick={() =>
            measure("Jump to last passage", () =>
              list.current?.reveal("passage-9999"),
            )
          }
        >
          Jump to last passage
        </button>
        <button
          onClick={() => measure("Toggle filter", () => setFiltered((v) => !v))}
        >
          Toggle filter
        </button>
        <button
          onClick={() => setTheme((v) => (v === "dark" ? "light" : "dark"))}
        >
          Toggle appearance
        </button>
      </div>
      <p role="status">
        {selected.size} selected · {filtered ? "Filtered" : "All passages"} ·{" "}
        {theme}
      </p>
      <output aria-label="Interaction measurements">
        {timings.join(" · ")}
      </output>
      <VirtualPassages
        ref={list}
        segments={
          filtered
            ? data.detail.segments.filter((_, i) => i % 20 === 0)
            : data.detail.segments
        }
        speakers={names}
        manual={manual}
        selected={selected}
        activeId={active}
        onSelect={select}
        onAssign={assign}
        onSeek={seek}
        onManualScroll={noop}
      />
    </main>
  );
}
const root = createRoot(document.getElementById("root")!);
root.render(<Benchmark />);
if (import.meta.hot) import.meta.hot.dispose(() => root.unmount());
