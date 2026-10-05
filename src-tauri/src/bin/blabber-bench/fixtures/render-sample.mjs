// Renders the report template with the sample fixture, the same way the Rust CLI does.
//   node src-tauri/src/bin/blabber-bench/fixtures/render-sample.mjs
// Writes fixtures/sample-report.html (full, with text) and fixtures/sample-summary.html
// (the shareable summary variant: textIncluded = false, no text/alignment/reference/terms/audio).
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const PLACEHOLDER = "/*__BENCH_DATA__*/null";
const template = readFileSync(join(here, "..", "report_template.html"), "utf8");
const count = template.split(PLACEHOLDER).length - 1;
if (count !== 1) throw new Error(`template must contain the placeholder exactly once, found ${count}`);

const results = JSON.parse(readFileSync(join(here, "sample-results.json"), "utf8"));

// Transcripts can contain "</script>"; escaping "<" keeps the JSON valid and the script block intact.
// The Rust side should do the same: serde_json::to_string(&results)?.replace('<', "\\u003c").
const embed = (data) => JSON.stringify(data).replace(/</g, "\\u003c");
const render = (data) => template.replace(PLACEHOLDER, () => embed(data));

function stripText(full) {
  const r = structuredClone(full);
  r.textIncluded = false;
  for (const c of r.clips) { delete c.reference; delete c.segments; delete c.terms; delete c.audioHref; }
  for (const row of r.results) { delete row.text; delete row.alignment; }
  return r;
}

writeFileSync(join(here, "sample-report.html"), render(results));
writeFileSync(join(here, "sample-summary.html"), render(stripText(results)));
console.log("wrote fixtures/sample-report.html and fixtures/sample-summary.html");
