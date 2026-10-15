// Re-spaces the messages of an Archify sequence diagram.
// Usage: node doc/diagrams/scripts/respace.mjs <source.json> [...]
//
// The ORDER of the messages in the "messages" array is the diagram's order; the "y" is recalculated
// here (fixed step). To insert a message, put it in the right position of the array with
// any "y"; if there are "segments", use a y equal to a neighboring message's in the phase
// it belongs to. The script also rebuilds the "segments" bands and the "meta.viewBox".
import { readFileSync, writeFileSync } from 'node:fs';

const STEP = 28;         // Archify's minimum: 28 px between messages that share space
const PHASE_GAP = 8;     // extra breathing room between phases (segments)
const TOP = 172;         // first y (Archify's minimum: 160)
const WIDTH = 1080;      // viewBox width (minimum screen ratio: ~1.55)

for (const file of process.argv.slice(2)) {
  const j = JSON.parse(readFileSync(file, 'utf8'));
  if (j.diagram_type !== 'sequence') { console.log(`${file}: skipped (not a sequence)`); continue; }

  const segs = j.segments || [];
  let phase = 0;
  const phases = j.messages.map((m) => {
    const i = segs.findIndex((s) => m.y >= s.from && m.y <= s.to);
    if (i >= 0) phase = i;
    return phase;
  });

  j.messages.forEach((m, i) => { m.y = TOP + i * STEP + phases[i] * PHASE_GAP; });
  segs.forEach((s, k) => {
    const ys = j.messages.filter((_, i) => phases[i] === k).map((m) => m.y);
    if (ys.length) { s.from = Math.min(...ys) - 14; s.to = Math.max(...ys) + 14; }
  });

  const last = j.messages[j.messages.length - 1].y;
  const height = Math.max(480, last + (segs.length ? 14 + 130 : 120));
  j.meta.viewBox = [WIDTH, height];
  writeFileSync(file, JSON.stringify(j, null, 2) + '\n');

  const warning = height > WIDTH / 1.55
    ? "  WARNING: diagram too tall for Archify's minimum ratio; split it in two diagrams."
    : '';
  console.log(`${file}: ${j.messages.length} messages, viewBox ${WIDTH}x${height}${warning}`);
}
