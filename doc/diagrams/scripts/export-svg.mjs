// Exports the SVG of an Archify diagram (generated HTML) using headless Chrome.
// Usage: node export-svg.mjs <input.html> <output.svg>
// Chrome: the ARCHIFY_CHROME variable, or the usual Chrome/Edge paths.
// No dependencies: uses Node's built-in WebSocket (>= 22).
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [input, output] = process.argv.slice(2);
if (!input || !output) {
  console.error('Usage: node export-svg.mjs <input.html> <output.svg>');
  process.exit(2);
}

const candidates = [
  process.env.ARCHIFY_CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
].filter(Boolean);
const chrome = candidates.find((c) => existsSync(c));
if (!chrome) {
  console.error('Chrome/Edge not found. Set ARCHIFY_CHROME to the executable path.');
  process.exit(2);
}

const profile = mkdtempSync(join(tmpdir(), 'archify-svg-'));
const proc = spawn(chrome, [
  '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
  `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });

const shutdown = () => {
  try { proc.kill(); } catch {}
  setTimeout(() => { try { rmSync(profile, { recursive: true, force: true }); } catch {} }, 500);
};

const port = await new Promise((ok, fail) => {
  let buf = '';
  const t = setTimeout(() => fail(new Error('Chrome did not start in time')), 30000);
  proc.stderr.on('data', (d) => {
    buf += d;
    const m = buf.match(/DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//);
    if (m) { clearTimeout(t); ok(m[1]); }
  });
  proc.on('exit', () => fail(new Error('Chrome exited earlier than expected')));
});

try {
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  let seq = 0;
  const pending = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  };
  const cdp = (method, params = {}) => new Promise((ok) => {
    const id = ++seq; pending.set(id, ok); ws.send(JSON.stringify({ id, method, params }));
  });

  await cdp('Page.enable');
  await cdp('Page.navigate', { url: pathToFileURL(resolve(input)).href });
  await new Promise((ok) => setTimeout(ok, 2500)); // load, fonts and layout

  // Captures the blob the viewer would generate on the "SVG · Auto" download (light/dark theme from the host).
  const expr = `(async () => {
    let blob = null;
    const orig = URL.createObjectURL;
    URL.createObjectURL = (b) => { blob = b; return orig.call(URL, b); };
    HTMLAnchorElement.prototype.click = function () {};
    Archify.exportMenu.run('svg');
    for (let i = 0; i < 100 && !blob; i++) await new Promise((r) => setTimeout(r, 50));
    return blob ? await blob.text() : null;
  })()`;
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  const svg = r.result?.result?.value;
  if (!svg || !svg.includes('<svg')) {
    console.error('Failed to export the SVG of', input, JSON.stringify(r.result?.exceptionDetails || r.result?.result));
    process.exitCode = 1;
  } else {
    writeFileSync(output, svg);
    console.log(`SVG: ${output} (${Math.round(svg.length / 1024)} KB)`);
  }
  ws.close();
} finally {
  shutdown();
}
