// Testes de bench/run.sh que não precisam de Docker: o plano (--dry-run), as recusas e a forma de cada sistema.
//   node --test bench/run.test.mjs
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const run = (...args) => spawnSync('bash', [join(here, 'run.sh'), ...args], { encoding: 'utf8' });
const systems = readdirSync(join(here, 'systems'), { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);

/// UC: UC-BNC-001
test('o dry-run imprime o plano, a estimativa e a ordem em rodízio, sem tocar no Docker', () => {
  const r = run('--dry-run', '--profile', 'standard', '--systems', 'taba,kafka,nats');
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /sistemas:\s+taba kafka nats/);
  assert.match(r.stdout, /tempo estimado:/);
  assert.match(r.stdout, /rodada 1: ordem taba kafka nats/);
  assert.match(r.stdout, /rodada 2: ordem kafka nats taba/);
  assert.match(r.stdout, /rodada 3: ordem nats taba kafka/);
  assert.match(r.stdout, /não é número absoluto/);
});

/// UC: UC-BNC-001
test('o servidor recebe o mesmo limite para todos os sistemas e o executor fica com o resto', () => {
  const r = run('--dry-run', '--cpus', '2', '--memory', '3g');
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /servidor:\s+2 núcleos, 3 GB \(igual para todos\)/);
});

/// UC: UC-BNC-001
test('um sistema, um perfil, um workload ou uma durabilidade desconhecidos são recusados', () => {
  for (const args of [['--systems', 'inexistente'], ['--profile', 'inexistente'], ['--workloads', 'inexistente'], ['--durability', 'nunca']]) {
    const r = run('--dry-run', ...args);
    assert.notEqual(r.status, 0, args.join(' '));
    assert.match(r.stderr, /ERRO|perfil desconhecido/);
  }
});

/// UC: UC-BNC-002
test('cada pasta de bench/systems é um sistema completo, com imagem fixada e os dois níveis de durabilidade', () => {
  assert.ok(systems.length >= 5, systems.join());
  for (const s of systems) {
    const dir = join(here, 'systems', s);
    const compose = readFileSync(join(dir, 'compose.yml'), 'utf8');
    assert.match(compose, /^  server:/m, `${s}: o serviço se chama server`);
    assert.match(compose, /healthcheck:/, `${s}: espera de saúde`);
    assert.match(compose, /cpus: \$\{BENCH_CPUS\}/, `${s}: limite de CPU do benchmark`);
    assert.match(compose, /mem_limit: \$\{BENCH_MEMORY\}/, `${s}: limite de memória do benchmark`);
    const image = /image: (\S+)/.exec(compose)[1];
    if (s !== 'taba') assert.doesNotMatch(image, /:latest$|^[^:]+$/, `${s}: imagem sem versão fixa (${image})`);
    for (const d of ['default', 'fsync']) {
      assert.ok(existsSync(join(dir, `driver-${d}.yaml`)), `${s}: driver-${d}.yaml`);
      assert.ok(existsSync(join(dir, `env-${d}`)), `${s}: env-${d}`);
    }
    assert.ok(existsSync(join(dir, 'README.md')), `${s}: README.md`);
  }
});

/// UC: UC-BNC-002
test('um sistema novo é encontrado pela pasta, sem mudar o run.sh', () => {
  for (const s of systems) {
    const r = run('--dry-run', '--systems', s);
    assert.equal(r.status, 0, `${s}: ${r.stderr}`);
    assert.match(r.stdout, new RegExp(String.raw`sistemas:\s+${s}\b`));
  }
});

/// UC: UC-BNC-002
test('os seis workloads usam a duração e o aquecimento do perfil', () => {
  const names = ['simple-1kb', 'max-rate-1kb', 'fanout-1kb', 'keyed-1kb', 'small-100b', 'backlog-1kb'];
  for (const w of names) {
    const text = readFileSync(join(here, 'workloads', `${w}.yaml`), 'utf8');
    assert.match(text, /testDurationMinutes: @DURATION@/, w);
    assert.match(text, /warmupDurationMinutes: @WARMUP@/, w);
    assert.match(text, /payloadFile: "payload\/payload-\d+(Kb|b)\.data"/, w);
  }
  const standard = readFileSync(join(here, 'profile-standard'), 'utf8');
  for (const w of names) assert.ok(standard.includes(w), `o perfil standard roda ${w}`);
  assert.match(readFileSync(join(here, 'profile-quick'), 'utf8'), /ROUNDS=1/);
});

/// UC: UC-BNC-005
test('o perfil consume roda só os workloads de backlog e todos os sistemas leem o backlog com confirmação manual', () => {
  const r = run('--dry-run', '--profile', 'consume');
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /workloads:\s+backlog-1kb backlog-100b backlog-1kb-4c/);
  for (const w of ['backlog-1kb', 'backlog-100b', 'backlog-1kb-4c']) {
    const text = readFileSync(join(here, 'workloads', `${w}.yaml`), 'utf8');
    assert.match(text, /consumerBacklogSizeGB: 1\b/, w);
    assert.match(text, /testDurationMinutes: @DURATION@/, w);
  }
  // Com 4 consumidores, 4 partições: sem elas o Kafka deixaria três consumidores parados.
  const four = readFileSync(join(here, 'workloads', 'backlog-1kb-4c.yaml'), 'utf8');
  assert.match(four, /partitionsPerTopic: 4/);
  assert.match(four, /consumerPerSubscription: 4/);
  // Kafka e Redpanda confirmam por offset: o commit automático fica desligado, e o relatório diz a diferença.
  for (const s of ['kafka', 'redpanda']) {
    assert.match(readFileSync(join(here, 'systems', s, 'driver-default.yaml'), 'utf8'), /enable\.auto\.commit=false/, s);
  }
});
