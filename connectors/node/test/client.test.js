'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { TabaClient, TabaError, VERSION, CONTRACT_VERSION } = require('../index');
const { consumeCall } = require('../src/client');
const { ScriptedServer, blockingSleep, withClient, json, topicJson, messageJson } = require('./helpers');

function code(promise) {
  return promise.then(
    () => assert.fail('an error was expected'),
    (e) => {
      assert.ok(e instanceof TabaError, `expected a TabaError, got ${e}`);
      return e;
    },
  );
}

function query(url) {
  return new URL(url, 'http://x').searchParams;
}

/// UC: UC-CNT-001, UC-CNT-029
test('the nine operations build the expected routes', () =>
  withClient({ token: 'tk' }, async (client, server) => {
    server.fallback(json(200, topicJson('orders')));
    await client.createTopic({ name: 'orders', type: 'queue' });
    await client.getTopic('orders');
    server.reply(json(200, { items: [topicJson('orders')] }));
    await client.listTopics({ limit: 10, cursor: 'c1', name: 'ord' });
    await client.updateTopic('orders', { idleTimeout: 60 });
    await client.deleteTopic('orders');
    server.reply(json(200, { id: 'm1' }));
    await client.publish('orders', Buffer.from('{"a":1}'), { groupId: 'g', deduplicationId: 'd' });
    server.reply(json(200, { messages: [] }));
    await client.consume('orders', { maxMessages: 2 });
    await client.ack('orders', { id: 'm 1', receiptToken: 'rt' });
    server.reply(json(200, { id: 'm1', invisibleUntil: 'x' }));
    await client.changeVisibility('orders', { id: 'm1', receiptToken: 'rt' }, 30);

    const seen = server.requests.map((r) => `${r.method} ${r.url}`);
    assert.deepEqual(seen, [
      'POST /topics',
      'GET /topics/orders',
      'GET /topics?limit=10&cursor=c1&name=ord',
      'PATCH /topics/orders',
      'DELETE /topics/orders',
      'POST /topics/orders/messages?groupId=g&deduplicationId=d',
      'GET /topics/orders/messages?maxMessages=2',
      'DELETE /topics/orders/messages/m%201?receiptToken=rt',
      'PUT /topics/orders/messages/m1/visibility?receiptToken=rt',
    ]);
    assert.equal(server.requests[8].bodyText, '{"visibilityTimeout":30}');
  }));

/// UC: UC-CNT-029
test('update topic is a real PATCH without a method override header', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, topicJson('orders')));
    await client.updateTopic('orders', { ttl: null, maxDeliveries: 5, tags: { a: 'b' }, prefixes: ['x-'] });
    const request = server.requests[0];
    assert.equal(request.method, 'PATCH');
    assert.equal(request.headers['x-http-method-override'], undefined);
    assert.deepEqual(JSON.parse(request.bodyText), { ttl: null, maxDeliveries: 5, tags: { a: 'b' }, prefixes: ['x-'] });
    assert.equal(request.headers['content-type'], 'application/json');
  }));

/// UC: UC-CNT-001
test('create topic sends only what was filled', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(201, topicJson('orders')));
    await client.createTopic({ name: 'orders', type: 'queue' });
    await client.createTopic({ name: 'd', type: 'distributor', prefixes: ['p-'], fifo: false, ttl: 30, tags: {} });
    assert.equal(server.requests[0].bodyText, '{"name":"orders","type":"queue"}');
    assert.deepEqual(JSON.parse(server.requests[1].bodyText), {
      name: 'd', type: 'distributor', prefixes: ['p-'], fifo: false, ttl: 30,
    });
  }));

/// UC: UC-CNT-002
test('config defaults and required base url', async () => {
  for (const bad of [undefined, '', '  ', 'ftp://x', 'not a url']) {
    assert.throws(() => new TabaClient({ baseUrl: bad }), (e) => e instanceof TabaError && e.code === 'client.invalid-config' && e.status === 0);
  }
  assert.throws(() => new TabaClient(), (e) => e.code === 'client.invalid-config');
  const client = new TabaClient({ baseUrl: 'http://localhost:8080/' });
  assert.equal(client._config.connectTimeout, 5000);
  assert.equal(client._config.requestTimeout, 30000);
  assert.equal(client._config.maxAttempts, 3);
  assert.equal(client._config.userAgent, `taba-node/${VERSION}`);
  client.close();
});

/// UC: UC-CNT-003
test('a token goes in every call and without it no call carries authorization', async () => {
  await withClient({ token: 'secret' }, async (client, server) => {
    server.fallback(json(200, topicJson('a')));
    await client.getTopic('a');
    assert.equal(server.requests[0].headers.authorization, 'Bearer secret');
  });
  await withClient({}, async (client, server) => {
    server.fallback(json(200, topicJson('a')));
    await client.getTopic('a');
    assert.equal(server.requests[0].headers.authorization, undefined);
  });
});

/// UC: UC-CNT-018
test('headers carry the agent and the optional ids', () =>
  withClient({ userAgent: 'my-app taba-node/9' }, async (client, server) => {
    server.fallback(json(200, { id: 'm1' }));
    await client.publish('q', Buffer.from('x'), { requestId: 'req-1', traceparent: '00-abc-def-01' });
    await client.publish('q', Buffer.from('x'));
    const [first, second] = server.requests;
    assert.equal(first.headers.accept, 'application/json');
    assert.equal(first.headers['user-agent'], 'my-app taba-node/9');
    assert.equal(first.headers['x-request-id'], 'req-1');
    assert.equal(first.headers.traceparent, '00-abc-def-01');
    assert.equal(second.headers['x-request-id'], undefined);
    assert.equal(second.headers.traceparent, undefined);
  }));

/// UC: UC-CNT-004, UC-CNT-025
test('publish sends raw bytes with content type and query', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { id: 'm1', duplicate: true }));
    const bytes = Buffer.from([0, 1, 2, 255, 254]);
    const reply = await client.publish('q', bytes, { contentType: 'application/octet-stream', groupId: 'g1', deduplicationId: 'd1' });
    assert.equal(reply.id, 'm1');
    assert.equal(reply.duplicate, true);
    const request = server.requests[0];
    assert.deepEqual(request.body, bytes);
    assert.equal(request.headers['content-type'], 'application/octet-stream');
    assert.equal(query(request.url).get('groupId'), 'g1');
    assert.equal(query(request.url).get('deduplicationId'), 'd1');

    await client.publishText('q', 'olá ✓');
    assert.equal(server.requests[1].headers['content-type'], 'text/plain; charset=utf-8');
    assert.equal(server.requests[1].bodyText, 'olá ✓');
    await client.publishRawJson('q', '{"n": 1.0}');
    assert.equal(server.requests[2].headers['content-type'], 'application/json');
    assert.equal(server.requests[2].bodyText, '{"n": 1.0}');
    await client.publish('q', Buffer.from('{}'));
    assert.equal(server.requests[3].headers['content-type'], 'application/json');
  }));

/// UC: UC-CNT-041
test('the batches send one JSON and read one result per item', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { results: [
      { status: 201, id: 'm1' },
      { status: 200, id: 'm0', duplicate: true },
      { status: 400, error: { code: 'validation.invalid-content-type', message: 'bad' } },
    ] }));
    const results = await client.publishBatch('q', [
      { body: Buffer.from([0, 1, 255]), contentType: 'application/octet-stream', groupId: 'g1', deduplicationId: 'd1' },
      { body: 'olá', contentType: 'text/plain; charset=utf-8' },
      { body: '{}', contentType: 'nope' },
    ]);
    let request = server.requests[0];
    assert.equal(request.method, 'POST');
    assert.ok(request.url.startsWith('/topics/q/messages/batch'), request.url);
    assert.equal(request.bodyText, '{"messages":[{"body":"AAH/","bodyEncoding":"base64","contentType":"application/octet-stream",'
      + '"groupId":"g1","deduplicationId":"d1"},'
      + '{"body":"b2zDoQ==","bodyEncoding":"base64","contentType":"text/plain; charset=utf-8"},'
      + '{"body":"e30=","bodyEncoding":"base64","contentType":"nope"}]}');
    assert.equal(results[0].ok, true);
    assert.equal(results[0].status, 201);
    assert.equal(results[0].id, 'm1');
    assert.equal(results[1].duplicate, true);
    assert.equal(results[2].ok, false);
    assert.equal(results[2].errorCode, 'validation.invalid-content-type');

    const messages = [{ id: 'm1', receiptToken: 'rt-m1' }, { id: 'm2', receiptToken: 'rt-m2' }];
    server.fallback(json(200, { results: [
      { status: 204, id: 'm1' },
      { status: 409, error: { code: 'message.stale-receipt', message: 'old' } },
    ] }));
    const acked = await client.ackBatch('q', messages);
    request = server.requests[1];
    assert.ok(request.url.startsWith('/topics/q/messages/ack'), request.url);
    assert.equal(request.bodyText, '{"messages":[{"id":"m1","receiptToken":"rt-m1"},{"id":"m2","receiptToken":"rt-m2"}]}');
    assert.equal(acked[0].ok, true);
    assert.equal(acked[1].errorCode, 'message.stale-receipt');

    server.fallback(json(200, { results: [{ status: 200, id: 'm1', invisibleUntil: '2026-10-29T10:00:00Z' }] }));
    const changed = await client.changeVisibilityBatch('q', messages.slice(0, 1), 30);
    request = server.requests[2];
    assert.ok(request.url.startsWith('/topics/q/messages/visibility'), request.url);
    assert.equal(request.bodyText, '{"messages":[{"id":"m1","receiptToken":"rt-m1","visibilityTimeout":30}]}');
    assert.equal(changed[0].invisibleUntil, '2026-10-29T10:00:00Z');
  }));

/// UC: UC-CNT-004
test('a distributor answer lists each copy and the partial failures', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, {
      matched: 2,
      deliveries: [
        { topic: 'a-1', id: 'm1' },
        { topic: 'a-2', error: { code: 'queue.full', message: 'full' } },
      ],
    }));
    const reply = await client.publish('dist', Buffer.from('{}'));
    assert.equal(reply.id, null);
    assert.equal(reply.matched, 2);
    assert.equal(reply.deliveries.length, 2);
    assert.equal(reply.deliveries[0].succeeded, true);
    assert.equal(reply.deliveries[0].id, 'm1');
    assert.equal(reply.deliveries[1].succeeded, false);
    assert.equal(reply.deliveries[1].errorCode, 'queue.full');
    assert.equal(reply.deliveries[1].errorMessage, 'full');
  }));

/// UC: UC-CNT-005
test('consume sends only what was given', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { messages: [] }));
    assert.deepEqual(await client.consume('q'), []);
    await client.consume('q', { maxMessages: 5, visibilityTimeout: 60, waitTime: 15 });
    assert.equal(server.requests[0].url, '/topics/q/messages');
    assert.equal(server.requests[1].url, '/topics/q/messages?maxMessages=5&visibilityTimeout=60&waitTime=15');
    await client.consume('q', { autoCreate: true, volatile: true, idleTimeout: 30, fifo: false, tags: { env: 'dev' } });
    const params = query(server.requests[2].url);
    assert.equal(params.get('autoCreate'), 'true');
    assert.equal(params.get('volatile'), 'true');
    assert.equal(params.get('idleTimeout'), '30');
    assert.equal(params.get('fifo'), 'false');
    assert.equal(params.get('tag.env'), 'dev');
  }));

/// UC: UC-CNT-005
test('auto create parameters are left out without auto create', () => {
  const c = consumeCall('q', { volatile: true, idleTimeout: 30, fifo: true, tags: { a: 'b' } });
  assert.deepEqual(c.query, []);
});

/// UC: UC-CNT-008
test('the consume read timeout is wait time plus ten seconds and not the request timeout', () => {
  assert.equal(consumeCall('q', { waitTime: 20 }).timeout, 30000);
  assert.equal(consumeCall('q', { waitTime: 25 }).timeout, 35000);
  assert.equal(consumeCall('q', {}).timeout, 10000);
});

/// UC: UC-CNT-006
test('ack and visibility put the receipt token in the query', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { id: 'm1', invisibleUntil: '2026-10-27T10:05:00Z' }));
    await client.ack('q', { id: 'm1', receiptToken: 'a b&c=d' });
    const visibility = await client.changeVisibility('q', { id: 'm1', receiptToken: 'rt' }, 45);
    assert.equal(query(server.requests[0].url).get('receiptToken'), 'a b&c=d');
    assert.equal(query(server.requests[1].url).get('receiptToken'), 'rt');
    assert.equal(visibility.invisibleUntil, '2026-10-27T10:05:00Z');
  }));

/// UC: UC-CNT-007
test('list all topics follows next cursor until the last page', () =>
  withClient({}, async (client, server) => {
    server.reply(
      json(200, { items: [topicJson('a'), topicJson('b')], nextCursor: 'c2' }),
      json(200, { items: [topicJson('c')], nextCursor: null }),
    );
    const all = await client.listAllTopics({ name: 'x' });
    assert.deepEqual(all.map((t) => t.name), ['a', 'b', 'c']);
    assert.equal(query(server.requests[0].url).get('cursor'), null);
    assert.equal(query(server.requests[1].url).get('cursor'), 'c2');
    assert.equal(query(server.requests[1].url).get('name'), 'x');
  }));

/// UC: UC-CNT-009, UC-CNT-017
test('a timeout is not repeated and is a client side error', () =>
  withClient({ requestTimeout: 150 }, async (client, server) => {
    server.fallback({ hang: true });
    const error = await code(client.getTopic('a'));
    assert.equal(error.status, 0);
    assert.equal(error.code, 'client.timeout');
    assert.equal(error.isClientSide, true);
    assert.equal(server.requests.length, 1);
  }));

/// UC: UC-CNT-010
test('retries 429 and 503 waiting for retry after then backoff', () =>
  withClient({}, async (client, server, sleep) => {
    server.reply(
      json(429, { code: 'rate.limited', message: 'slow' }, { 'Retry-After': '7' }),
      json(503, { code: 'node.unavailable', message: 'x' }, { 'Retry-After': '2' }),
      json(200, topicJson('a')),
    );
    const topic = await client.getTopic('a');
    assert.equal(topic.name, 'a');
    assert.equal(server.requests.length, 3);
    assert.deepEqual(sleep.waits, [7000, 2000]);
  }));

/// UC: UC-CNT-010
test('backoff without retry after is one two four seconds up to max attempts', async () => {
  await withClient({ maxAttempts: 4 }, async (client, server, sleep) => {
    server.fallback(json(503, { code: 'node.unavailable', message: 'x' }));
    const error = await code(client.getTopic('a'));
    assert.equal(error.status, 503);
    assert.equal(server.requests.length, 4);
    assert.deepEqual(sleep.waits, [1000, 2000, 4000]);
  });
  await withClient({ maxAttempts: 1 }, async (client, server, sleep) => {
    server.fallback(json(429, { code: 'rate.limited', message: 'x' }));
    await code(client.getTopic('a'));
    assert.equal(server.requests.length, 1);
    assert.deepEqual(sleep.waits, []);
  });
});

/// UC: UC-CNT-011
test('after a 503 a 409 on create and a 404 on delete count as success', () =>
  withClient({}, async (client, server) => {
    server.reply(
      json(503, { code: 'node.unavailable', message: 'x' }, { 'Retry-After': '0' }),
      json(409, { code: 'topic.already-exists', message: 'exists' }),
      json(200, topicJson('orders')),
      json(503, { code: 'node.unavailable', message: 'x' }, { 'Retry-After': '0' }),
      json(404, { code: 'topic.not-found', message: 'gone' }),
    );
    const topic = await client.createTopic({ name: 'orders', type: 'queue' });
    assert.equal(topic.name, 'orders');
    assert.deepEqual(server.requests.map((r) => r.method), ['POST', 'POST', 'GET']);
    await client.deleteTopic('orders');
    assert.equal(server.requests.length, 5);
  }));

/// UC: UC-CNT-011, UC-CNT-014
test('without a previous 503 a 409 or a 404 is a plain error', () =>
  withClient({}, async (client, server) => {
    server.reply(
      json(409, { code: 'topic.already-exists', message: 'exists' }),
      json(404, { code: 'topic.not-found', message: 'gone' }),
    );
    assert.equal((await code(client.createTopic({ name: 'a', type: 'queue' }))).code, 'topic.already-exists');
    assert.equal((await code(client.deleteTopic('a'))).code, 'topic.not-found');
    assert.equal(server.requests.length, 2);
  }));

/// UC: UC-CNT-012
test('gateway errors are only repeated when publishing to a fifo', async () => {
  await withClient({}, async (client, server) => {
    server.reply(json(502, 'bad gateway'), json(504, 'timeout'), json(200, { id: 'm1' }));
    const reply = await client.publish('q', Buffer.from('{}'), { fifo: true, deduplicationId: 'd' });
    assert.equal(reply.id, 'm1');
    assert.equal(server.requests.length, 3);
  });
  await withClient({}, async (client, server) => {
    server.fallback(json(502, 'bad gateway'));
    assert.equal((await code(client.publish('q', Buffer.from('{}')))).status, 502);
    assert.equal(server.requests.length, 1);
    server.fallback(json(504, 'timeout'));
    assert.equal((await code(client.getTopic('a'))).status, 504);
    assert.equal(server.requests.length, 2);
  });
});

/// UC: UC-CNT-013
test('network failure before sending is repeated and after is not', async () => {
  // Connection closed after the request was sent: it is not known whether it was applied.
  await withClient({}, async (client, server) => {
    server.reply({ destroy: true });
    const error = await code(client.publish('q', Buffer.from('{}')));
    assert.equal(error.code, 'client.network');
    assert.equal(error.status, 0);
    assert.equal(server.requests.length, 1);
  });
  // Connection refused: the body never left, so the call is repeated.
  const server = await ScriptedServer.start();
  const baseUrl = server.url;
  await server.close();
  const waits = [];
  const client = new TabaClient({ baseUrl, maxAttempts: 3 }, { sleep: async (ms) => waits.push(ms) });
  const error = await code(client.publish('q', Buffer.from('{}')));
  assert.equal(error.code, 'client.network');
  assert.deepEqual(waits, [1000, 2000]);
  client.close();
});

/// UC: UC-CNT-013
test('a refused connection is repeated and then succeeds when the node comes back', async () => {
  const server = await ScriptedServer.start();
  server.fallback(json(200, topicJson('a')));
  const port = server.port;
  await server.close();
  const waits = [];
  const sleep = async (ms) => {
    waits.push(ms);
    // The node comes back on the same port during the wait.
    if (!server._server.listening) {
      await new Promise((resolve) => server._server.listen(port, '127.0.0.1', resolve));
    }
  };
  const client = new TabaClient({ baseUrl: `http://127.0.0.1:${port}` }, { sleep });
  assert.equal((await client.getTopic('a')).name, 'a');
  assert.deepEqual(waits, [1000]);
  client.close();
  await server.close();
});

/// UC: UC-CNT-014
test('a 4xx other than 429 is never repeated', () =>
  withClient({}, async (client, server, sleep) => {
    for (const status of [400, 401, 403, 404, 409, 422]) {
      server.reply(json(status, { code: `x.${status}`, message: 'no' }));
      const error = await code(client.getTopic('a'));
      assert.equal(error.status, status);
    }
    assert.equal(server.requests.length, 6);
    assert.deepEqual(sleep.waits, []);
  }));

/// UC: UC-CNT-017, UC-CNT-018
test('the error carries the error response fields and the headers', () =>
  withClient({}, async (client, server) => {
    server.reply(json(
      422,
      { code: 'topic.invalid-name', message: 'bad name', requestId: 'body-id', details: [{ field: 'name', reason: 'x' }] },
      { 'Retry-After': '9', 'X-Request-Id': 'header-id' },
    ));
    const error = await code(client.createTopic({ name: '!', type: 'queue' }));
    assert.equal(error.status, 422);
    assert.equal(error.code, 'topic.invalid-name');
    assert.equal(error.message, 'bad name');
    assert.equal(error.requestId, 'body-id');
    assert.equal(error.retryAfter, 9);
    assert.deepEqual(JSON.parse(error.details), [{ field: 'name', reason: 'x' }]);
    assert.ok(error.body.length > 0);

    server.reply(json(400, { code: 'x.y', message: 'm' }, { 'X-Request-Id': 'header-id' }));
    assert.equal((await code(client.getTopic('a'))).requestId, 'header-id');
  }));

/// UC: UC-CNT-017
test('a non json error still reports a status code', () =>
  withClient({}, async (client, server) => {
    server.reply(json(500, '<html>boom</html>', { 'Content-Type': 'text/html' }), json(500, ''));
    const html = await code(client.getTopic('a'));
    assert.equal(html.code, 'http.500');
    assert.equal(html.message, '<html>boom</html>');
    const empty = await code(client.getTopic('a'));
    assert.equal(empty.code, 'http.500');
    assert.match(empty.message, /no body/);
  }));

/// UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
test('messages come back as bytes for json text and base64', () =>
  withClient({}, async (client, server) => {
    server.reply({
      status: 200,
      body: '{"messages":[' + [
        '{"id":"m1","body":{"pedido": 9001, "valor": 1.0, "grande": 12345678901234567890},"contentType":"application/json","receiptToken":"rt-m1","deliveryCount":3,"publishedAt":"p","invisibleUntil":"i","groupId":"g","deduplicationId":"d","novo":{"campo":[1,2]}}',
        '{"id":"m2","body":"olá ✓ \\u00e9","contentType":"text/plain; charset=utf-8","receiptToken":"rt-m2"}',
        '{"id":"m3","body":"AAEC/w==","contentType":"application/octet-stream","bodyEncoding":"base64","receiptToken":"rt-m3"}',
        '{"id":"m4","body":{"a":1},"contentType":"application/vnd.taba+json","receiptToken":"rt-m4","deadLetteredAt":"d","deadLetterReason":"max-deliveries"}',
      ].join(',') + ']}',
    });
    const [m1, m2, m3, m4] = await client.consume('q');
    // The JSON text comes exactly as the server sent it, not re-serialized.
    assert.equal(m1.bodyText, '{"pedido": 9001, "valor": 1.0, "grande": 12345678901234567890}');
    assert.ok(Buffer.isBuffer(m1.body));
    assert.equal(m1.deliveryCount, 3);
    assert.equal(m1.groupId, 'g');
    assert.equal(m1.deduplicationId, 'd');
    assert.equal(m1.receiptToken, 'rt-m1');
    assert.equal(m1.publishedAt, 'p');
    assert.equal(m1.invisibleUntil, 'i');
    assert.equal(m1.deadLetteredAt, null);
    assert.equal(m2.bodyText, 'olá ✓ é');
    assert.equal(m2.contentType, 'text/plain; charset=utf-8');
    assert.deepEqual(m3.body, Buffer.from([0, 1, 2, 255]));
    assert.equal(m4.bodyText, '{"a":1}');
    assert.equal(m4.deadLetteredAt, 'd');
    assert.equal(m4.deadLetterReason, 'max-deliveries');
    assert.deepEqual(m4.json(), { a: 1 });
  }));

/// UC: UC-CNT-026
test('an invalid answer is an invalid response error and not a crash', () =>
  withClient({}, async (client, server) => {
    server.reply(
      json(200, 'not json'),
      json(200, '{"messages":[{"id":"m1","body":"AAA","bodyEncoding":"base64"}]}'),
      json(200, '{"messages":[{"id":"m1","body":"x"'),
      json(200, '[1,2]'),
    );
    assert.equal((await code(client.getTopic('a'))).code, 'client.invalid-response');
    assert.equal((await code(client.consume('q'))).code, 'client.invalid-response');
    assert.equal((await code(client.consume('q'))).code, 'client.invalid-response');
    assert.equal((await code(client.getTopic('a'))).code, 'client.invalid-response');
  }));

/// UC: UC-CNT-028
test('unknown fields in responses are ignored and the contract version is declared', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { ...topicJson('a'), novo: { campo: [1, { x: 'y' }] }, outro: null }));
    const topic = await client.getTopic('a');
    assert.equal(topic.name, 'a');
    assert.equal(CONTRACT_VERSION, '1');
  }));

/// UC: UC-CNT-020
test('many concurrent calls can use one client at the same time', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, topicJson('a')));
    const topics = await Promise.all(Array.from({ length: 50 }, () => client.getTopic('a')));
    assert.equal(topics.length, 50);
    assert.equal(server.requests.length, 50);
  }));

/// UC: UC-CNT-032
test('every operation is asynchronous and returns a promise', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { items: [], messages: [], id: 'x', name: 'a' }));
    const m = { id: 'm', receiptToken: 'r' };
    const calls = [
      client.createTopic({ name: 'a', type: 'queue' }),
      client.getTopic('a'),
      client.listTopics(),
      client.listAllTopics(),
      client.updateTopic('a', {}),
      client.deleteTopic('a'),
      client.publish('a', Buffer.from('x')),
      client.publishJson('a', { x: 1 }),
      client.consume('a'),
      client.ack('a', m),
      client.changeVisibility('a', m, 5),
    ];
    for (const result of calls) {
      assert.ok(result instanceof Promise);
    }
    await Promise.all(calls);
  }));

/// UC: UC-CNT-020
test('aborting the signal cancels the call without a client error', () =>
  withClient({}, async (client, server) => {
    server.fallback({ hang: true });
    const controller = new AbortController();
    const pending = client.consume('q', { waitTime: 20, signal: controller.signal });
    setTimeout(() => controller.abort(), 50);
    await assert.rejects(pending, (e) => !(e instanceof TabaError) && e.name === 'AbortError');
    assert.equal(server.requests.length, 1);

    await assert.rejects(client.getTopic('a', { signal: AbortSignal.abort() }), (e) => e.name === 'AbortError');
    assert.equal(server.requests.length, 1);
  }));

/// UC: UC-CNT-031
test('subscribe acks only what the handler accepted and stops on abort', () =>
  withClient({}, async (client, server) => {
    const controller = new AbortController();
    server.batches([messageJson('m1', { n: 1 })], [messageJson('m2', { n: 2 })], [messageJson('m3', { n: 3 })]);
    const handled = [];
    const failures = [];
    await client.subscribe('q', undefined, async (message) => {
      handled.push(message.id);
      if (message.id === 'm2') {
        throw new Error('boom');
      }
      if (message.id === 'm3') {
        controller.abort();
      }
    }, { signal: controller.signal, onError: (message, error) => failures.push([message.id, error.message]) });

    assert.deepEqual(handled, ['m1', 'm2', 'm3']);
    assert.deepEqual(failures, [['m2', 'boom']]);
    const acks = server.requests.filter((r) => r.method === 'DELETE').map((r) => r.url);
    assert.deepEqual(acks, ['/topics/q/messages/m1?receiptToken=rt-m1', '/topics/q/messages/m3?receiptToken=rt-m3']);
    const first = query(server.requests[0].url);
    assert.equal(first.get('waitTime'), '20');
    assert.equal(first.get('maxMessages'), '1');
  }));

/// UC: UC-CNT-031
test('subscribe ends without an error when aborted during the long polling', () =>
  withClient({}, async (client, server) => {
    server.fallback({ hang: true });
    const controller = new AbortController();
    setTimeout(() => controller.abort(), 50);
    await client.subscribe('q', undefined, async () => {}, { signal: controller.signal });
    assert.equal(server.requests.length, 1);
  }));

/// UC: UC-CNT-033
test('publish json serializes the object and sends it as json', () =>
  withClient({}, async (client, server) => {
    server.fallback(json(200, { id: 'm1' }));
    await client.publishJson('q', { pedido: 9001, itens: ['a'] });
    assert.equal(server.requests[0].headers['content-type'], 'application/json');
    assert.equal(server.requests[0].bodyText, '{"pedido":9001,"itens":["a"]}');

    const own = new TabaClient({ baseUrl: server.url, serializer: { stringify: () => '{"own":true}', parse: JSON.parse } });
    await own.publishJson('q', { x: 1 });
    assert.equal(server.requests[1].bodyText, '{"own":true}');
    own.close();

    assert.throws(() => client.publishJson('q', null), (e) => e.code === 'client.invalid-request');
    await code(Promise.resolve().then(() => client.publishJson('q', { big: 10n })));
  }));

/// UC: UC-CNT-034
test('consume with a handler deletes by default and keeps the message when asked or when it fails', () =>
  withClient({}, async (client, server) => {
    client._transport.sleep = blockingSleep;
    server.batches([messageJson('m1', { n: 1 })], [messageJson('m2', { n: 2 })], [messageJson('m3', { n: 3 })]);
    assert.equal(await client.consumeWith('q', undefined, async (context) => {
      assert.equal(context.message.id, 'm1');
      assert.equal(context.delete, true);
    }), 1);
    assert.equal(await client.consumeWith('q', undefined, async (context) => {
      context.delete = false;
    }), 1);
    await assert.rejects(client.consumeWith('q', undefined, async () => {
      throw new Error('handler failed');
    }), /handler failed/);

    assert.deepEqual(server.requests.map((r) => r.method), ['GET', 'DELETE', 'GET', 'GET']);
    const poll = query(server.requests[0].url);
    assert.equal(poll.get('maxMessages'), '1');
    assert.equal(poll.get('visibilityTimeout'), '120');
    assert.equal(poll.get('waitTime'), '20');
  }));

/// UC: UC-CNT-034
test('consume with a handler renews the visibility while it runs and lets it change it', async () => {
  const server = await ScriptedServer.start();
  server.reply(json(200, { messages: [messageJson('m1', { n: 1 })] }));
  server.fallback(json(200, { id: 'm1', invisibleUntil: 'x' }));
  // Each wait of the renewal ends only when the test says so (a "tick").
  const pending = [];
  const sleep = (ms, signal) => new Promise((resolve, reject) => {
    pending.push(resolve);
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
  });
  const tick = async () => {
    while (pending.length === 0) {
      await new Promise((resolve) => setTimeout(resolve, 2));
    }
    pending.shift()();
  };
  const client = new TabaClient({ baseUrl: server.url }, { sleep });
  const renewals = () => server.requests.filter((r) => r.method === 'PUT');
  const until = async (condition) => {
    while (!condition()) {
      await new Promise((resolve) => setTimeout(resolve, 2));
    }
  };
  await client.consumeWith('q', { visibilityTimeout: 90 }, async (context) => {
    await tick();
    await tick();
    await until(() => renewals().length === 2);
    assert.equal(context.leaseSeconds, 90);
    await context.changeVisibility(300);
    assert.equal(context.leaseSeconds, 300);
    await tick();
    await tick();
    await until(() => renewals().length === 5);
  });
  const bodies = renewals().map((r) => JSON.parse(r.bodyText).visibilityTimeout);
  assert.deepEqual(bodies, [90, 90, 300, 300, 300]);
  assert.equal(server.requests[server.requests.length - 1].method, 'DELETE');
  client.close();
  await server.close();
});

/// UC: UC-CNT-034
test('a refused renewal marks the lease as lost and the delete is tolerated', async () => {
  const server = await ScriptedServer.start();
  server.reply(
    json(200, { messages: [messageJson('m1', { n: 1 })] }),
    json(409, { code: 'message.receipt-expired', message: 'x' }),
  );
  server.fallback(json(409, { code: 'message.receipt-expired', message: 'x' }));
  let rounds = 0;
  const sleep = (ms, signal) => (rounds++ < 1
    ? Promise.resolve()
    : new Promise((resolve, reject) => signal.addEventListener('abort', () => reject(signal.reason), { once: true })));
  const client = new TabaClient({ baseUrl: server.url }, { sleep });
  let lost = false;
  await client.consumeWith('q', undefined, async (context) => {
    while (!context.leaseLost) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    lost = context.leaseLost;
  });
  assert.equal(lost, true);
  client.close();
  await server.close();
});

/// UC: UC-CNT-034
test('consume json hands over the parsed body and refuses a body that is not json', () =>
  withClient({}, async (client, server) => {
    client._transport.sleep = blockingSleep;
    server.batches([messageJson('m1', { pedido: 9001 })], [messageJson('m2', 'not json', { contentType: 'text/plain' })]);
    await client.consumeJson('q', undefined, async (context) => {
      assert.deepEqual(context.body, { pedido: 9001 });
      assert.equal(context.message.id, 'm1');
    });
    const error = await code(client.consumeJson('q', undefined, async () => assert.fail('the handler must not run')));
    assert.equal(error.code, 'client.invalid-response');
    assert.deepEqual(server.requests.map((r) => r.method), ['GET', 'DELETE', 'GET']);
  }));
