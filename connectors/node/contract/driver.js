'use strict';

// The Node.js driver of the connector contract tests (connectors/contract/README.md): reads the scenario files
// and runs each line against a real Taba through the connector, in the style of the language.
//
//   node contract/driver.js --url http://host:8080 --token <token> --prefix ct-node-0101 --lang node \
//        --scenario ../contract/scenarios/core.scn

const fs = require('node:fs');
const { TabaClient, TabaError } = require('..');

const args = parseArgs(process.argv.slice(2));

function parseArgs(argv) {
  const out = { scenarios: [] };
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, '');
    if (key === 'scenario') out.scenarios.push(argv[i + 1]);
    else out[key] = argv[i + 1];
  }
  return out;
}

// ---- the scenario language ------------------------------------------------------------------------------

function percentDecode(text) {
  return text.replace(/(?:%[0-9A-Fa-f]{2})+/g, (run) => Buffer.from(run.replace(/%/g, ''), 'hex').toString('utf8'));
}

// Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched.
function expand(value, vars) {
  let out = '';
  let last = 0;
  const re = /\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)/g;
  for (let m = re.exec(value); m !== null; m = re.exec(value)) {
    out += percentDecode(value.slice(last, m.index));
    if (m[0] === '{p}') out += args.prefix;
    else if (m[0] === '{lang}') out += args.lang;
    else out += (vars[m[1]] || {})[m[2]] ?? '';
    last = m.index + m[0].length;
  }
  return out + percentDecode(value.slice(last));
}

function parseLine(text, vars) {
  const [head, tail] = text.split(' => ');
  const [op, ...pairs] = head.trim().split(/\s+/);
  const params = {};
  for (const pair of pairs) {
    const i = pair.indexOf('=');
    params[pair.slice(0, i)] = expand(pair.slice(i + 1), vars);
  }
  const expects = [];
  for (const token of (tail || '').trim().split(/\s+/).filter(Boolean)) {
    const i = token.search(/[=~]/);
    expects.push({ key: token.slice(0, i), op: token[i], value: expand(token.slice(i + 1), vars) });
  }
  return { op, params, expects };
}

// ---- the operations -------------------------------------------------------------------------------------

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const num = (v) => (v === undefined ? undefined : Number(v));
const flag = (v) => (v === undefined ? undefined : v === 'true');
const list = (v) => (v === undefined ? undefined : v.split(',').filter(Boolean));
const tags = (v) => (v === undefined ? undefined : Object.fromEntries(v.split(',').map((t) => t.split(/:(.*)/s).slice(0, 2))));
// "null" goes back to the server default; absent leaves the field out.
const nullable = (v) => (v === undefined ? undefined : v === 'null' ? null : Number(v));

function render(message) {
  const type = message.contentType.split(';')[0].trim().toLowerCase();
  return type === 'application/octet-stream' ? 'b64:' + message.body.toString('base64') : message.bodyText;
}

function topicFields(t) {
  return { id: t.id, name: t.name, type: t.type, fifo: t.fifo === null ? '' : String(t.fifo), volatile: t.volatile === null ? '' : String(t.volatile), raw: t.raw };
}

function publishFields(r) {
  return { id: r.id ?? '', duplicate: r.duplicate === null ? '' : String(r.duplicate), matched: r.matched === null ? '' : String(r.matched) };
}

function consumeOptions(p) {
  return { maxMessages: num(p.max), waitTime: num(p.wait), visibilityTimeout: num(p.visibility) };
}

function messageFields(messages) {
  const first = messages[0];
  const fields = { count: String(messages.length), bodies: messages.map(render).join(',') };
  if (first) {
    Object.assign(fields, { id: first.id, receiptToken: first.receiptToken, deliveryCount: String(first.deliveryCount), contentType: first.contentType, body: render(first) });
  }
  return fields;
}

const operations = {
  async createTopic(c, p) {
    return topicFields(await c.createTopic({
      name: p.name, type: p.type, fifo: flag(p.fifo), prefixes: list(p.prefixes), tags: tags(p.tags), volatile: flag(p.volatile),
      ttl: num(p.ttl), maxDeliveries: num(p.maxDeliveries), maxMessageBytes: num(p.maxMessageBytes), idleTimeout: num(p.idleTimeout),
    }));
  },
  async getTopic(c, p) { return topicFields(await c.getTopic(p.name)); },
  async listTopics(c, p) {
    const topics = await c.listAllTopics({ name: p.name });
    return { count: String(topics.length), names: topics.map((t) => t.name).sort().join(',') };
  },
  async updateTopic(c, p) {
    return topicFields(await c.updateTopic(p.name, {
      prefixes: list(p.prefixes), tags: tags(p.tags), idleTimeout: num(p.idleTimeout),
      ttl: nullable(p.ttl), maxDeliveries: nullable(p.maxDeliveries), maxMessageBytes: nullable(p.maxMessageBytes),
    }));
  },
  async deleteTopic(c, p) { await c.deleteTopic(p.name); return {}; },

  async publishText(c, p) {
    return publishFields(await c.publishText(p.topic, p.body, { groupId: p.groupId, deduplicationId: p.deduplicationId, fifo: flag(p.fifo) }));
  },
  async publishJson(c, p) {
    return publishFields(await c.publishRawJson(p.topic, p.body, { groupId: p.groupId, deduplicationId: p.deduplicationId, fifo: flag(p.fifo) }));
  },
  async publishBytes(c, p) {
    const options = { contentType: p.contentType || 'application/octet-stream', groupId: p.groupId, deduplicationId: p.deduplicationId, fifo: flag(p.fifo) };
    return publishFields(await c.publish(p.topic, Buffer.from(p.base64, 'base64'), options));
  },

  // A batch of texts (one item per text); bad=true adds an item with an invalid content type.
  async publishBatch(c, p) {
    const items = list(p.texts).map((text) => ({ body: text, contentType: 'text/plain; charset=utf-8', groupId: p.groupId }));
    if (flag(p.bad)) items.push({ body: 'x', contentType: 'not a type' });
    const results = await c.publishBatch(p.topic, items);
    return { count: String(results.length), statuses: results.map((r) => r.status).join(',') };
  },
  // Consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes the receipt of
  // the first one old before the batch: it is given back and taken again.
  async settleBatch(c, p) {
    const messages = await c.consume(p.topic, consumeOptions(p));
    if (flag(p.stale) && messages.length > 0) {
      await c.changeVisibility(p.topic, messages[0], 0);
      await c.consume(p.topic, { maxMessages: 1, waitTime: 2, visibilityTimeout: num(p.visibility) });
    }
    const results = p.action === 'visibility'
      ? await c.changeVisibilityBatch(p.topic, messages, Number(p.seconds))
      : await c.ackBatch(p.topic, messages);
    return { count: String(results.length), statuses: results.map((r) => r.status).join(',') };
  },

  async consume(c, p) { return messageFields(await c.consume(p.topic, consumeOptions(p))); },
  async ack(c, p) { await c.ack(p.topic, { id: p.id, receiptToken: p.receipt }); return {}; },
  async changeVisibility(c, p) {
    const r = await c.changeVisibility(p.topic, { id: p.id, receiptToken: p.receipt }, Number(p.seconds));
    return { id: r.id, invisibleUntil: r.invisibleUntil };
  },

  // Consuming with a handler: mode ok (deleted), fail (the handler throws), keep (delete = false) or slow (holds the
  // message for `hold` seconds, which only works if the invisibility is renewed).
  async handle(c, p) {
    const seen = [];
    let leaseLost = false;
    let failed = false;
    let handled = 0;
    try {
      handled = await c.consumeWith(p.topic, consumeOptions(p), async (context) => {
        seen.push(render(context.message));
        if (p.mode === 'fail') throw new Error('the handler failed');
        if (p.mode === 'keep') context.delete = false;
        if (p.mode === 'slow') await sleep(Number(p.hold) * 1000);
        leaseLost = context.leaseLost;
      });
    } catch (e) {
      if (e instanceof TabaError) throw e;
      failed = true;
    }
    return { handled: String(handled), failed: String(failed), bodies: seen.join(','), leaseLost: String(leaseLost) };
  },

  // The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first one fail.
  async subscribe(c, p) {
    const controller = new AbortController();
    const seen = [];
    const timer = setTimeout(() => controller.abort(), Number(p.timeout) * 1000);
    let calls = 0;
    await c.subscribe(p.topic, { visibilityTimeout: num(p.visibility) }, async (message) => {
      calls += 1;
      seen.push(render(message));
      if (seen.length >= Number(p.count)) controller.abort();
      if (p.failFirst === 'true' && calls === 1) throw new Error('the handler failed');
    }, { signal: controller.signal });
    clearTimeout(timer);
    return { count: String(seen.length), bodies: seen.join(',') };
  },

  // A long polling cancelled from outside must end at once, not at the end of the wait.
  async cancelConsume(c, p) {
    const controller = new AbortController();
    const started = Date.now();
    setTimeout(() => controller.abort(), Number(p.after) * 1000);
    let cancelled = false;
    try {
      await c.consume(p.topic, { waitTime: 20, signal: controller.signal });
    } catch (e) {
      cancelled = controller.signal.aborted;
      if (!cancelled) throw e;
    }
    return { cancelled: String(cancelled), fast: String(Date.now() - started < 10000) };
  },

  // Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
  async drain(c, p) {
    const seen = [];
    const deadline = Date.now() + Number(p.timeout) * 1000;
    while (seen.length < Number(p.count) && Date.now() < deadline) {
      for (const message of await c.consume(p.topic, { maxMessages: 10, waitTime: 1, visibilityTimeout: 60 })) {
        seen.push(render(message));
        await c.ack(p.topic, message);
      }
    }
    return { count: String(seen.length), bodies: seen.sort().join(',') };
  },

  async sleep(c, p) { await sleep(Number(p.seconds) * 1000); return {}; },
};

// ---- the run --------------------------------------------------------------------------------------------

function check(result, expects) {
  const problems = [];
  const failure = result.error !== undefined;
  if (failure && !expects.some((e) => e.key === 'error')) problems.push(`unexpected error ${result.error} (status ${result.status}): ${result.message}`);
  if (!failure && expects.some((e) => e.key === 'error')) problems.push('expected an error, but the call succeeded');
  for (const { key, op, value } of expects) {
    const actual = result[key];
    const mismatch = op === '~' ? !(actual || '').includes(value) : value === '*' ? !actual : actual !== value;
    if (mismatch) {
      problems.push(`${key} ${op} ${JSON.stringify(value)}, got ${JSON.stringify(actual)}`);
    }
  }
  return problems;
}

async function main() {
  const clients = new Map();
  const clientFor = (p) => {
    const key = `${p.url || ''}|${p.token || ''}|${p.attempts || ''}`;
    if (!clients.has(key)) {
      clients.set(key, new TabaClient({ baseUrl: p.url || args.url, token: p.token || args.token, maxAttempts: p.attempts ? Number(p.attempts) : undefined }));
    }
    return clients.get(key);
  };
  const vars = {};
  let passed = 0;
  let failed = 0;
  let skipped = 0;
  for (const file of args.scenarios) {
    const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
    for (let n = 0; n < lines.length; n += 1) {
      const text = lines[n].trim();
      if (text === '' || text.startsWith('#')) continue;
      const where = `${file.split('/').pop()}:${n + 1}`;
      const { op, params, expects } = parseLine(text, vars);
      if (!operations[op]) {
        skipped += 1;
        console.log(`SKIP ${where} ${op} (not supported by this connector)`);
        continue;
      }
      let result;
      try {
        result = await operations[op](clientFor(params), params);
      } catch (e) {
        if (!(e instanceof TabaError)) throw e;
        result = { error: e.code, status: String(e.status), message: e.message };
      }
      if (params.as) vars[params.as] = result;
      const problems = check(result, expects);
      if (problems.length === 0) {
        passed += 1;
        console.log(`PASS ${where} ${op}`);
      } else {
        failed += 1;
        console.log(`FAIL ${where} ${text}`);
        for (const problem of problems) console.log(`       ${problem}`);
      }
    }
  }
  for (const client of clients.values()) client.close();
  console.log(`${args.lang}: ${passed} passed, ${failed} failed, ${skipped} skipped`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(e);
  process.exit(2);
});
