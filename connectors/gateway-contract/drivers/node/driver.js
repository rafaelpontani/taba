// The Node.js driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
// runs each line through @aws-sdk/client-sqs and client-sns, the way an application would. Prints one `RESULT` line per
// scenario line.
//
//   node driver.js --sqs-endpoint http://host:9324 --sns-endpoint http://host:9324 --endpoint-mode env|code \
//       --prefix gw-node-0101 --lang node --variant current --scenario file.scn ...
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const net = require('net');
const sqsLib = require('@aws-sdk/client-sqs');
const snsLib = require('@aws-sdk/client-sns');

const ARGS = { scenario: [], cred: [] };
for (let i = 2; i < process.argv.length; i += 2) {
  const key = process.argv[i].slice(2);
  if (key in ARGS && Array.isArray(ARGS[key])) ARGS[key].push(process.argv[i + 1]);
  else ARGS[key] = process.argv[i + 1];
}
const REGION = ARGS.region || 'us-east-1';
const CREDS = {};
for (const item of ARGS.cred) {
  const [name, pair] = item.split(/=(.*)/s);
  const [ak, sk] = pair.split(/:(.*)/s);
  CREDS[name] = { accessKeyId: ak, secretAccessKey: sk };
}

function reachable(url) {
  const u = new URL(url);
  return new Promise((resolve) => {
    const s = net.connect({ host: u.hostname, port: Number(u.port || 80), timeout: 3000 });
    s.on('connect', () => { s.destroy(); resolve(true); });
    s.on('error', () => resolve(false));
    s.on('timeout', () => { s.destroy(); resolve(false); });
  });
}

let sqs, sns;
function makeClients(credentials) {
  const base = { region: REGION, maxAttempts: 1 };
  if (credentials) base.credentials = credentials;
  const code = ARGS['endpoint-mode'] === 'code';
  sqs = new sqsLib.SQSClient(code ? { ...base, endpoint: ARGS['sqs-endpoint'] } : base);
  sns = new snsLib.SNSClient(code ? { ...base, endpoint: ARGS['sns-endpoint'] } : base);
}
makeClients();

// ---- the scenario language ---------------------------------------------------------------------------------------------

function unquote(s) { return decodeURIComponent(s.replace(/%(?![0-9A-Fa-f]{2})/g, '%25')); }
const VARIABLE = /\{p\}|\{lang\}|\{prev\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)/g;

function expand(value, vars) {
  let out = '';
  let last = 0;
  for (const m of value.matchAll(VARIABLE)) {
    out += unquote(value.slice(last, m.index));
    if (m[0] === '{p}') out += ARGS.prefix;
    else if (m[0] === '{lang}') out += ARGS.lang;
    else if (m[0] === '{prev}') out += ARGS.prev || '';
    else out += String(((vars[m[1]] || {})[m[2]]) ?? '');
    last = m.index + m[0].length;
  }
  return out + unquote(value.slice(last));
}

function parseLine(text, vars) {
  const at = text.indexOf(' => ');
  const head = at < 0 ? text : text.slice(0, at);
  const tail = at < 0 ? '' : text.slice(at + 4);
  const parts = head.split(/\s+/);
  const params = {};
  for (const part of parts.slice(1)) {
    const eq = part.indexOf('=');
    params[part.slice(0, eq)] = expand(part.slice(eq + 1), vars);
  }
  const expects = [];
  for (const token of tail.split(/\s+/).filter(Boolean)) {
    const m = /^([A-Za-z0-9_.]+)(>=|<=|!=|=|~)(.*)$/s.exec(token);
    expects.push([m[1], m[2], expand(m[3], vars)]);
  }
  return { op: parts[0], params, expects };
}

function check(result, expects) {
  for (const [key, op, want] of expects) {
    const got = String(result[key] ?? '');
    let ok;
    if (op === '=' && want === '*') ok = got !== '';
    else if (op === '=') ok = got === want;
    else if (op === '!=') ok = got !== want;
    else if (op === '~') ok = got.includes(want);
    else if (op === '>=') ok = Number(got) >= Number(want);
    else ok = Number(got) <= Number(want);
    if (!ok) return `${key}${op}${want} but was '${got.slice(0, 200)}'`;
  }
  return null;
}

// ---- the operations ----------------------------------------------------------------------------------------------------

const md5 = (s) => crypto.createHash('md5').update(s, 'utf8').digest('hex');
const pairs = (t) => (t ? Object.fromEntries(t.split(',').map((i) => { const k = i.indexOf(':'); return [i.slice(0, k), i.slice(k + 1)]; })) : {});

function msgAttrsIn(text) {
  const out = {};
  for (const item of text ? text.split(',') : []) {
    const a = item.indexOf(':');
    const b = item.indexOf(':', a + 1);
    const name = item.slice(0, a), type = item.slice(a + 1, b), value = item.slice(b + 1);
    out[name] = type === 'Binary' ? { DataType: type, BinaryValue: Buffer.from(value.slice(4), 'base64') } : { DataType: type, StringValue: value };
  }
  return out;
}

function msgAttrsOut(message, result) {
  for (const [name, a] of Object.entries(message.MessageAttributes || {})) {
    result[`msgattr_${name}`] = a.DataType.startsWith('Binary') ? `b64:${Buffer.from(a.BinaryValue).toString('base64')}` : a.StringValue;
    result[`msgattrtype_${name}`] = a.DataType;
  }
}

const namesOf = (urls) => urls.map((u) => u.replace(/\/+$/, '').split('/').pop()).sort();

function received(messages, elapsed) {
  const r = {
    count: messages.length, elapsedMs: elapsed,
    bodies: messages.map((m) => m.Body).join(','), sorted: messages.map((m) => m.Body).sort().join(','),
    _msgs: messages.map((m) => ({ handle: m.ReceiptHandle, id: m.MessageId })),
  };
  if (messages.length) {
    const m = messages[0];
    Object.assign(r, { body: m.Body, id: m.MessageId, handle: m.ReceiptHandle, md5: m.MD5OfBody, md5Ok: String(md5(m.Body) === m.MD5OfBody), size: m.Body.length });
    for (const [k, v] of Object.entries(m.Attributes || {})) r[`attr_${k}`] = v;
    msgAttrsOut(m, r);
    try {
      const doc = JSON.parse(m.Body);
      if (doc && typeof doc === 'object' && !Array.isArray(doc)) for (const [k, v] of Object.entries(doc)) if (typeof v === 'string') r[`json_${k}`] = v;
    } catch (e) { /* not JSON */ }
  }
  return r;
}

const sent = (r, body) => ({ id: r.MessageId, md5: r.MD5OfMessageBody, md5Ok: String(md5(body) === r.MD5OfMessageBody), sequence: r.SequenceNumber || '' });
const batchResult = (r) => ({ ok: (r.Successful || []).length, failed: (r.Failed || []).length });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function sendArgs(p, body) {
  const a = { MessageBody: body };
  if (p.delay) a.DelaySeconds = Number(p.delay);
  if (p.msgattrs) a.MessageAttributes = msgAttrsIn(p.msgattrs);
  if (p.group) a.MessageGroupId = p.group;
  if (p.dedup) a.MessageDeduplicationId = p.dedup;
  return a;
}

const handlesOf = (p, vars) => vars[p.from.replace(/^\$/, '')]._msgs.map((m) => m.handle);

const OPS = {
  async createQueue(p) {
    const attrs = pairs(p.attrs);
    if (p.fifo === 'true') attrs.FifoQueue = 'true';
    if (p.redriveArn) attrs.RedrivePolicy = JSON.stringify({ deadLetterTargetArn: p.redriveArn, maxReceiveCount: Number(p.redriveMax) });
    const a = { QueueName: p.name };
    if (Object.keys(attrs).length) a.Attributes = attrs;
    return { url: (await sqs.send(new sqsLib.CreateQueueCommand(a))).QueueUrl, name: p.name };
  },
  async getQueueUrl(p) { return { url: (await sqs.send(new sqsLib.GetQueueUrlCommand({ QueueName: p.name }))).QueueUrl }; },
  async deleteQueue(p) { await sqs.send(new sqsLib.DeleteQueueCommand({ QueueUrl: p.queue })); return {}; },
  async purgeQueue(p) { await sqs.send(new sqsLib.PurgeQueueCommand({ QueueUrl: p.queue })); return {}; },
  async listQueues(p) {
    const urls = (await sqs.send(new sqsLib.ListQueuesCommand({ QueueNamePrefix: p.prefix }))).QueueUrls || [];
    return { count: urls.length, names: namesOf(urls).join(',') };
  },
  async getQueueAttributes(p) {
    const r = (await sqs.send(new sqsLib.GetQueueAttributesCommand({ QueueUrl: p.queue, AttributeNames: p.names.split(',') }))).Attributes || {};
    return Object.fromEntries(Object.entries(r).map(([k, v]) => [`attr_${k}`, v]));
  },
  async setQueueAttributes(p) { await sqs.send(new sqsLib.SetQueueAttributesCommand({ QueueUrl: p.queue, Attributes: pairs(p.attrs) })); return {}; },
  async tagQueue(p) { await sqs.send(new sqsLib.TagQueueCommand({ QueueUrl: p.queue, Tags: pairs(p.tags) })); return {}; },
  async untagQueue(p) { await sqs.send(new sqsLib.UntagQueueCommand({ QueueUrl: p.queue, TagKeys: p.keys.split(',') })); return {}; },
  async listQueueTags(p) {
    const r = (await sqs.send(new sqsLib.ListQueueTagsCommand({ QueueUrl: p.queue }))).Tags || {};
    return Object.fromEntries(Object.entries(r).map(([k, v]) => [`tag_${k}`, v]));
  },
  async send(p) { return sent(await sqs.send(new sqsLib.SendMessageCommand({ QueueUrl: p.queue, ...sendArgs(p, p.body) })), p.body); },
  async sendBig(p) {
    const body = 'x'.repeat(Number(p.size));
    return sent(await sqs.send(new sqsLib.SendMessageCommand({ QueueUrl: p.queue, MessageBody: body })), body);
  },
  async sendBatch(p) {
    const delays = p.delays ? p.delays.split(',') : [];
    const entries = p.bodies.split(',').map((body, i) => {
      const e = { Id: String(i), MessageBody: body };
      if (delays.length) e.DelaySeconds = Number(delays[i]);
      if (p.group) e.MessageGroupId = p.group;
      if (p.dedupPrefix) e.MessageDeduplicationId = p.dedupPrefix + i;
      return e;
    });
    return batchResult(await sqs.send(new sqsLib.SendMessageBatchCommand({ QueueUrl: p.queue, Entries: entries })));
  },
  async sendBatchTooMany(p) {
    const entries = Array.from({ length: Number(p.count) }, (_, i) => ({ Id: String(i), MessageBody: `m${i}` }));
    await sqs.send(new sqsLib.SendMessageBatchCommand({ QueueUrl: p.queue, Entries: entries }));
    return {};
  },
  async receive(p) {
    const a = { QueueUrl: p.queue, MaxNumberOfMessages: Number(p.max || 1), WaitTimeSeconds: Number(p.wait || 0) };
    if (p.visibility) a.VisibilityTimeout = Number(p.visibility);
    if (p.attrs) a.AttributeNames = p.attrs.split(',');
    if (p.msgattrs) a.MessageAttributeNames = p.msgattrs.split(',');
    const t0 = Date.now();
    const r = await sqs.send(new sqsLib.ReceiveMessageCommand(a));
    return received(r.Messages || [], Date.now() - t0);
  },
  async drainAll(p) {
    const want = Number(p.count), deadline = Date.now() + Number(p.timeout) * 1000;
    const got = [], result = {};
    while (got.length < want && Date.now() < deadline) {
      const a = { QueueUrl: p.queue, MaxNumberOfMessages: 10, WaitTimeSeconds: 1 };
      if (p.msgattrs) a.MessageAttributeNames = p.msgattrs.split(',');
      for (const m of (await sqs.send(new sqsLib.ReceiveMessageCommand(a))).Messages || []) {
        got.push(m);
        msgAttrsOut(m, result);
        await sqs.send(new sqsLib.DeleteMessageCommand({ QueueUrl: p.queue, ReceiptHandle: m.ReceiptHandle }));
      }
    }
    result.count = got.length;
    result.bodies = got.map((m) => m.Body).join(',');
    return result;
  },
  async deleteMessage(p) { await sqs.send(new sqsLib.DeleteMessageCommand({ QueueUrl: p.queue, ReceiptHandle: p.handle })); return {}; },
  async changeVisibility(p) {
    await sqs.send(new sqsLib.ChangeMessageVisibilityCommand({ QueueUrl: p.queue, ReceiptHandle: p.handle, VisibilityTimeout: Number(p.seconds) }));
    return {};
  },
  async deleteAll(p, vars) {
    const entries = handlesOf(p, vars).map((h, i) => ({ Id: String(i), ReceiptHandle: h }));
    return batchResult(await sqs.send(new sqsLib.DeleteMessageBatchCommand({ QueueUrl: p.queue, Entries: entries })));
  },
  async changeVisibilityAll(p, vars) {
    const entries = handlesOf(p, vars).map((h, i) => ({ Id: String(i), ReceiptHandle: h, VisibilityTimeout: Number(p.seconds) }));
    return batchResult(await sqs.send(new sqsLib.ChangeMessageVisibilityBatchCommand({ QueueUrl: p.queue, Entries: entries })));
  },
  async allowSnsToSend(p) {
    const policy = { Version: '2012-10-17', Statement: [{ Effect: 'Allow', Principal: { Service: 'sns.amazonaws.com' }, Action: 'sqs:SendMessage', Resource: p.queueArn, Condition: { ArnEquals: { 'aws:SourceArn': p.topic } } }] };
    await sqs.send(new sqsLib.SetQueueAttributesCommand({ QueueUrl: p.queue, Attributes: { Policy: JSON.stringify(policy) } }));
    return {};
  },
  async sleep(p) { await sleep(Number(p.seconds) * 1000); return {}; },
  async useCredentials(p) { makeClients(CREDS[p.name]); return {}; },
  async createTopic(p) {
    const a = { Name: p.name };
    if (p.fifo === 'true') a.Attributes = { FifoTopic: 'true' };
    return { arn: (await sns.send(new snsLib.CreateTopicCommand(a))).TopicArn };
  },
  async deleteTopic(p) { await sns.send(new snsLib.DeleteTopicCommand({ TopicArn: p.topic })); return {}; },
  async listTopics(p) {
    const arns = [];
    let token;
    do {
      const r = await sns.send(new snsLib.ListTopicsCommand(token ? { NextToken: token } : {}));
      arns.push(...(r.Topics || []).map((t) => t.TopicArn));
      token = r.NextToken;
    } while (token);
    const names = arns.map((a) => a.split(':').pop()).filter((n) => n.startsWith(p.prefix)).sort();
    return { count: names.length, names: names.join(',') };
  },
  async getTopicAttributes(p) {
    const r = (await sns.send(new snsLib.GetTopicAttributesCommand({ TopicArn: p.topic }))).Attributes || {};
    return Object.fromEntries(Object.entries(r).map(([k, v]) => [`attr_${k}`, v]));
  },
  async subscribe(p) {
    const a = { TopicArn: p.topic, Protocol: p.protocol, Endpoint: p.endpoint, ReturnSubscriptionArn: true };
    if (p.attrs) a.Attributes = pairs(p.attrs);
    return { arn: (await sns.send(new snsLib.SubscribeCommand(a))).SubscriptionArn };
  },
  async unsubscribe(p) { await sns.send(new snsLib.UnsubscribeCommand({ SubscriptionArn: p.sub })); return {}; },
  async listSubscriptions(p) {
    const subs = (await sns.send(new snsLib.ListSubscriptionsByTopicCommand({ TopicArn: p.topic }))).Subscriptions || [];
    return { count: subs.length, endpoints: subs.map((s) => s.Endpoint).sort().join(','), protocols: [...new Set(subs.map((s) => s.Protocol))].sort().join(',') };
  },
  async getSubscriptionAttributes(p) {
    const r = (await sns.send(new snsLib.GetSubscriptionAttributesCommand({ SubscriptionArn: p.sub }))).Attributes || {};
    return Object.fromEntries(Object.entries(r).map(([k, v]) => [`attr_${k}`, v]));
  },
  async setSubscriptionAttributes(p) {
    for (const [k, v] of Object.entries(pairs(p.attrs))) {
      await sns.send(new snsLib.SetSubscriptionAttributesCommand({ SubscriptionArn: p.sub, AttributeName: k, AttributeValue: v }));
    }
    return {};
  },
  async publish(p) {
    const a = { TopicArn: p.topic, Message: p.message };
    if (p.subject) a.Subject = p.subject;
    if (p.msgattrs) a.MessageAttributes = msgAttrsIn(p.msgattrs);
    if (p.group) a.MessageGroupId = p.group;
    if (p.dedup) a.MessageDeduplicationId = p.dedup;
    return { messageId: (await sns.send(new snsLib.PublishCommand(a))).MessageId };
  },
  async publishBatch(p) {
    const entries = p.messages.split(',').map((m, i) => ({ Id: String(i), Message: m }));
    return batchResult(await sns.send(new snsLib.PublishBatchCommand({ TopicArn: p.topic, PublishBatchRequestEntries: entries })));
  },
};

async function runFile(file, vars, unreachable) {
  const counts = { PASS: 0, FAIL: 0, CONNECT: 0 };
  const name = file.replace(/\\/g, '/').split('/').pop();
  const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
  for (let n = 0; n < lines.length; n++) {
    const text = lines[n].trim();
    if (!text || text.startsWith('#')) continue;
    let status = 'PASS', detail = '', opName = text.split(/\s+/)[0];
    try {
      const { op, params, expects } = parseLine(text, vars);
      opName = op;
      const expected = (expects.find(([k]) => k === 'error') || [])[2];
      try {
        const result = await OPS[op](params, vars);
        if (expected !== undefined) { status = 'FAIL'; detail = `expected error ${expected} but the call worked`; }
        else { const problem = check(result, expects); if (problem) { status = 'FAIL'; detail = problem; } }
        if (params.as) vars[params.as] = result;
      } catch (e) {
        if (e && e.$metadata && e.$metadata.httpStatusCode) {
          // The old SDKs (3.4xx) only keep the name of the modeled class (`InvalidParameterException`) for the wire code
          // `InvalidParameter`; the code behind it is what the scenarios compare.
          const codes = [e.name, e.Code, e.__type, e.code].filter(Boolean);
          if (e.name && e.name.endsWith('Exception')) codes.push(e.name.slice(0, -'Exception'.length));
          if (expected === undefined || !codes.some((c) => expected.split('|').includes(c))) {
            status = 'FAIL'; detail = `error ${codes.join('/')}: ${String(e.message).slice(0, 160)}`;
          }
        } else throw e;
      }
    } catch (e) {
      if (unreachable) { status = 'CONNECT'; detail = (e && (e.code || e.name)) || 'error'; }
      else { status = 'FAIL'; detail = `${e && e.name}: ${String(e && e.message).slice(0, 200)}`; }
    }
    counts[status]++;
    console.log(`RESULT ${status} ${name}:${n + 1} ${opName}${detail ? ' :: ' + detail : ''}`);
  }
  return counts;
}

(async () => {
  const unreachable = !(await reachable(ARGS['sqs-endpoint'])) || !(await reachable(ARGS['sns-endpoint']));
  const total = { PASS: 0, FAIL: 0, CONNECT: 0 };
  const vars = {};
  for (const file of ARGS.scenario) for (const [k, v] of Object.entries(await runFile(file, vars, unreachable))) total[k] += v;
  const version = require('@aws-sdk/client-sqs/package.json').version;
  console.log(`SUMMARY lang=${ARGS.lang} variant=${ARGS.variant || ''} sdk=client-sqs-${version} pass=${total.PASS} fail=${total.FAIL} connect=${total.CONNECT}`);
})();
