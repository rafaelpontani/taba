'use strict';

const http = require('node:http');
const { TabaClient } = require('../index');

/**
 * A local HTTP server that records every request and answers from a script: the equivalent of the
 * `ScriptedHandler` of the C# connector. A reply is `{status, headers, body}`, a function of the request
 * returning one, or `{destroy: true}` (closes the connection with no answer), `{hang: true}` (never answers).
 */
class ScriptedServer {
  constructor() {
    this.requests = [];
    this._script = [];
    this._fallback = { status: 200, body: '{}' };
    this._sockets = new Set();
    this._server = http.createServer((req, res) => this._handle(req, res));
    this._server.on('connection', (socket) => {
      this._sockets.add(socket);
      socket.on('close', () => this._sockets.delete(socket));
    });
  }

  static async start() {
    const server = new ScriptedServer();
    await new Promise((resolve) => server._server.listen(0, '127.0.0.1', resolve));
    return server;
  }

  get url() {
    return `http://127.0.0.1:${this._server.address().port}`;
  }

  get port() {
    return this._server.address().port;
  }

  reply(...replies) {
    this._script.push(...replies);
    return this;
  }

  /**
   * Answers the consume calls (GET) with these message batches, in order, then with an empty one;
   * every other call (ack, visibility) gets a plain 200.
   */
  batches(...batches) {
    const queue = batches.slice();
    this._fallback = (request) => {
      if (request.method !== 'GET') {
        return { status: 200, body: '{"id":"x","invisibleUntil":"x"}' };
      }
      return { status: 200, body: JSON.stringify({ messages: queue.length > 0 ? queue.shift() : [] }) };
    };
    return this;
  }

  fallback(reply) {
    this._fallback = reply;
    return this;
  }

  _handle(req, res) {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      const record = { method: req.method, url: req.url, headers: req.headers, body: Buffer.concat(chunks) };
      record.bodyText = record.body.toString('utf8');
      this.requests.push(record);
      let reply = this._script.length > 0 ? this._script.shift() : this._fallback;
      if (typeof reply === 'function') {
        reply = reply(record);
      }
      if (reply.destroy) {
        req.socket.destroy();
        return;
      }
      if (reply.hang) {
        return;
      }
      const body = reply.body === undefined ? '' : reply.body;
      res.writeHead(reply.status || 200, Object.assign({ 'Content-Type': 'application/json' }, reply.headers));
      res.end(body);
    });
  }

  async close() {
    for (const socket of this._sockets) {
      socket.destroy();
    }
    await new Promise((resolve) => this._server.close(resolve));
  }
}

/** A `sleep` that never ends by itself: the renewal does not spin in tests that do not look at it. */
function blockingSleep(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal) {
      signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    }
  });
}

/** A `sleep` that does not wait and records what was asked. */
function recordingSleep() {
  const waits = [];
  const sleep = async (ms) => {
    waits.push(ms);
  };
  sleep.waits = waits;
  return sleep;
}

async function withClient(config, fn, script) {
  const server = await ScriptedServer.start();
  if (script) {
    script(server);
  }
  const sleep = recordingSleep();
  const client = new TabaClient(Object.assign({ baseUrl: server.url }, config), { sleep });
  try {
    await fn(client, server, sleep);
  } finally {
    client.close();
    await server.close();
  }
}

function json(status, value, headers) {
  return { status, headers, body: typeof value === 'string' ? value : JSON.stringify(value) };
}

function topicJson(name, extra) {
  return Object.assign({ id: `t-${name}`, name, type: 'queue', createdAt: '2026-10-27T10:00:00Z' }, extra);
}

function messageJson(id, body, extra) {
  return Object.assign({
    id,
    body,
    contentType: 'application/json',
    receiptToken: `rt-${id}`,
    deliveryCount: 1,
    publishedAt: '2026-10-27T10:00:00Z',
    invisibleUntil: '2026-10-27T10:02:00Z',
  }, extra);
}

module.exports = { ScriptedServer, blockingSleep, recordingSleep, withClient, json, topicJson, messageJson };
