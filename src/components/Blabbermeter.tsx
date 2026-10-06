import { usePlayful } from "../lib/appearance";
import {
  formatCount,
  formatMinutesSaved,
  minutesSaved,
  wordComparison,
} from "../lib/blabbermeter";
import type { DictationStats } from "../types/domain";

/** How much you have blabbered: words, time saved and streak. Hidden in Serious mode. */
export function Blabbermeter({ stats }: { stats: DictationStats | null }) {
  const playful = usePlayful();
  if (!playful || !stats) return null;
  const saved = minutesSaved(stats.totalWords, stats.totalDurationMs);
  return (
    <section className="blabbermeter" aria-labelledby="blabbermeter-heading">
      <div className="blabbermeter-header">
        <h2 id="blabbermeter-heading" className="section-title">Blabbermeter</h2>
        <p className="blabbermeter-quip">{wordComparison(stats.totalWords)}</p>
      </div>
      {stats.totalWords > 0 ? (
        <dl className="blabbermeter-stats">
          <div>
            <dt>Today</dt>
            <dd>{formatCount(stats.todayWords)} <span>words</span></dd>
          </div>
          <div>
            <dt>All time</dt>
            <dd>{formatCount(stats.totalWords)} <span>words</span></dd>
          </div>
          <div title="Compared with typing at 40 words per minute">
            <dt>Time saved</dt>
            <dd>{formatMinutesSaved(saved)}</dd>
          </div>
          <div>
            <dt>Streak</dt>
            <dd>
              {stats.streakDays} <span>{stats.streakDays === 1 ? "day" : "days"}</span>
            </dd>
          </div>
        </dl>
      ) : null}
    </section>
  );
}
