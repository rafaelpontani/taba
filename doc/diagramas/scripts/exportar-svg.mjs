// Exporta o SVG de um diagrama do Archify (HTML gerado) usando o Chrome headless.
// Uso: node exportar-svg.mjs <entrada.html> <saida.svg>
// Chrome: variável ARCHIFY_CHROME, ou os caminhos comuns do Chrome/Edge.
// Sem dependências: usa o WebSocket embutido do Node (>= 22).
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [entrada, saida] = process.argv.slice(2);
if (!entrada || !saida) {
  console.error('Uso: node exportar-svg.mjs <entrada.html> <saida.svg>');
  process.exit(2);
}

const candidatos = [
  process.env.ARCHIFY_CHROME,
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/usr/bin/google-chrome',
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
].filter(Boolean);
const chrome = candidatos.find((c) => existsSync(c));
if (!chrome) {
  console.error('Chrome/Edge não encontrado. Defina ARCHIFY_CHROME com o caminho do executável.');
  process.exit(2);
}

const perfil = mkdtempSync(join(tmpdir(), 'archify-svg-'));
const proc = spawn(chrome, [
  '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
  `--user-data-dir=${perfil}`, '--remote-debugging-port=0', 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });

const encerrar = () => {
  try { proc.kill(); } catch {}
  setTimeout(() => { try { rmSync(perfil, { recursive: true, force: true }); } catch {} }, 500);
};

const porta = await new Promise((ok, erro) => {
  let buf = '';
  const t = setTimeout(() => erro(new Error('Chrome não iniciou a tempo')), 30000);
  proc.stderr.on('data', (d) => {
    buf += d;
    const m = buf.match(/DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//);
    if (m) { clearTimeout(t); ok(m[1]); }
  });
  proc.on('exit', () => erro(new Error('Chrome encerrou antes do esperado')));
});

try {
  const alvo = await (await fetch(`http://127.0.0.1:${porta}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(alvo.webSocketDebuggerUrl);
  await new Promise((ok, erro) => { ws.onopen = ok; ws.onerror = erro; });
  let seq = 0;
  const pend = new Map();
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pend.has(m.id)) { pend.get(m.id)(m); pend.delete(m.id); }
  };
  const cdp = (method, params = {}) => new Promise((ok) => {
    const id = ++seq; pend.set(id, ok); ws.send(JSON.stringify({ id, method, params }));
  });

  await cdp('Page.enable');
  await cdp('Page.navigate', { url: pathToFileURL(resolve(entrada)).href });
  await new Promise((ok) => setTimeout(ok, 2500)); // carga, fontes e layout

  // Captura o blob que o viewer geraria no download "SVG · Auto" (tema claro/escuro pelo host).
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
    console.error('Falha ao exportar o SVG de', entrada, JSON.stringify(r.result?.exceptionDetails || r.result?.result));
    process.exitCode = 1;
  } else {
    writeFileSync(saida, svg);
    console.log(`SVG: ${saida} (${Math.round(svg.length / 1024)} KB)`);
  }
  ws.close();
} finally {
  encerrar();
}
