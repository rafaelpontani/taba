'use strict';

const { TabaError } = require('./errors');
const { Transport, encode } = require('./transport');
const {
  MessageContext,
  parseMessages,
  parsePage,
  parsePublishResponse,
  parseTopic,
  parseVisibilityResponse,
} = require('./models');

/** The version of this connector. */
const VERSION = '0.1.0';

/** The version of the API contract this connector implements. */
const CONTRACT_VERSION = '1';

// `consume` waits `waitTime` plus this slack, not the `requestTimeout`.
const LONG_POLL_SLACK_MS = 10000;

const HANDLER_MAX_MESSAGES = 1;
const HANDLER_VISIBILITY_TIMEOUT = 120;
const HANDLER_WAIT_TIME = 20;

function isAbort(e, signal) {
  return signal !== undefined && signal.aborted && (e === signal.reason || (e && e.name === 'AbortError'));
}

function normalize(config) {
  if (config === null || typeof config !== 'object') {
    throw TabaError.client('client.invalid-config', 'config must be an object with at least baseUrl');
  }
  return {
    baseUrl: config.baseUrl,
    token: config.token,
    connectTimeout: config.connectTimeout !== undefined ? config.connectTimeout : 5000,
    requestTimeout: config.requestTimeout !== undefined ? config.requestTimeout : 30000,
    maxAttempts: config.maxAttempts !== undefined ? config.maxAttempts : 3,
    userAgent: config.userAgent !== undefined ? config.userAgent : `taba-node/${VERSION}`,
    serializer: config.serializer,
    agent: config.agent,
  };
}

function call(method, path) {
  return {
    method,
    path,
    query: [],
    body: null,
    contentType: undefined,
    requestId: undefined,
    traceparent: undefined,
    timeout: undefined,
    retryGateway: false,
    recover: [],
  };
}

function withQuery(c, key, value) {
  c.query.push([key, String(value)]);
  return c;
}

function withJson(c, text) {
  c.body = Buffer.from(text, 'utf8');
  return c;
}

function segment(value) {
  return encode(String(value));
}

function consumeCall(topic, options) {
  const o = options || {};
  const c = call('GET', `/topics/${segment(topic)}/messages`);
  if (o.maxMessages !== undefined) {
    withQuery(c, 'maxMessages', o.maxMessages);
  }
  if (o.visibilityTimeout !== undefined) {
    withQuery(c, 'visibilityTimeout', o.visibilityTimeout);
  }
  if (o.waitTime !== undefined) {
    withQuery(c, 'waitTime', o.waitTime);
  }
  if (o.autoCreate) {
    withQuery(c, 'autoCreate', true);
    if (o.volatile !== undefined) {
      withQuery(c, 'volatile', o.volatile);
    }
    if (o.idleTimeout !== undefined) {
      withQuery(c, 'idleTimeout', o.idleTimeout);
    }
    if (o.fifo !== undefined) {
      withQuery(c, 'fifo', o.fifo);
    }
    for (const [key, value] of Object.entries(o.tags || {})) {
      withQuery(c, `tag.${key}`, value);
    }
  }
  c.requestId = o.requestId;
  c.traceparent = o.traceparent;
  c.timeout = (o.waitTime || 0) * 1000 + LONG_POLL_SLACK_MS;
  return c;
}

function handlerDefaults(options) {
  const o = Object.assign({}, options);
  if (o.maxMessages === undefined) o.maxMessages = HANDLER_MAX_MESSAGES;
  if (o.visibilityTimeout === undefined) o.visibilityTimeout = HANDLER_VISIBILITY_TIMEOUT;
  if (o.waitTime === undefined) o.waitTime = HANDLER_WAIT_TIME;
  return o;
}

/** A third of the invisibility (and never all of it), so a failed renewal can be tried again in time. */
function renewalInterval(leaseSeconds) {
  return (Math.max(1, leaseSeconds) / 3) * 1000;
}

function createJson(request) {
  const out = { name: request.name || '', type: request.type || '' };
  if (request.tags && Object.keys(request.tags).length > 0) out.tags = request.tags;
  if (request.prefixes && request.prefixes.length > 0) out.prefixes = request.prefixes;
  for (const key of ['volatile', 'idleTimeout', 'fifo', 'maxMessageBytes', 'ttl', 'maxDeliveries']) {
    if (request[key] !== undefined && request[key] !== null) out[key] = request[key];
  }
  return JSON.stringify(out);
}

/** A PATCH sends what is set; `null` goes back to the server default and `undefined` is left out. */
function updateJson(update) {
  const out = {};
  for (const key of ['tags', 'prefixes', 'idleTimeout', 'maxMessageBytes', 'ttl', 'maxDeliveries']) {
    if (update[key] !== undefined) out[key] = update[key];
  }
  return JSON.stringify(out);
}

/**
 * The Taba client API client (doc 21): the nine operations, `subscribe` and the consume with a handler.
 * Everything is asynchronous (there is no synchronous HTTP in Node). A single object serves any number of
 * concurrent calls.
 */
class TabaClient {
  /**
   * @param {object} config `baseUrl` (required), `token`, `connectTimeout` (ms, 5000), `requestTimeout` (ms,
   *   30000), `maxAttempts` (3), `userAgent`, `serializer` (`{stringify, parse}`), `agent`.
   * @param {{sleep?: (ms: number, signal?: AbortSignal) => Promise<void>}} [internals] For tests.
   */
  constructor(config, internals) {
    this._config = normalize(config);
    this._transport = new Transport(this._config, internals && internals.sleep);
    this._serializer = this._config.serializer || JSON;
  }

  /** Releases the connections the client opened. */
  close() {
    this._transport.close();
  }

  _send(c, signal) {
    return this._transport.send(c, signal);
  }

  async createTopic(request, { signal } = {}) {
    if (!request) {
      throw TabaError.client('client.invalid-request', 'request is required');
    }
    const c = withJson(call('POST', '/topics'), createJson(request));
    c.recover.push([409, 'topic.already-exists']);
    const reply = await this._send(c, signal);
    if (reply.recovered) {
      return this.getTopic(request.name, { signal });
    }
    return parseTopic(reply.text);
  }

  async getTopic(name, { signal } = {}) {
    const reply = await this._send(call('GET', `/topics/${segment(name)}`), signal);
    return parseTopic(reply.text);
  }

  /** `GET /topics`. Parameters left `undefined` do not go in the query. */
  async listTopics({ limit, cursor, name, signal } = {}) {
    const c = call('GET', '/topics');
    if (limit !== undefined) withQuery(c, 'limit', limit);
    if (cursor !== undefined && cursor !== null) withQuery(c, 'cursor', cursor);
    if (name !== undefined) withQuery(c, 'name', name);
    const reply = await this._send(c, signal);
    return parsePage(reply.text);
  }

  /** All topics, page by page. */
  async listAllTopics({ name, signal } = {}) {
    const all = [];
    let cursor = null;
    do {
      const page = await this.listTopics({ cursor, name, signal });
      all.push(...page.items);
      cursor = page.nextCursor;
    } while (cursor !== null);
    return all;
  }

  async updateTopic(name, update, { signal } = {}) {
    if (!update) {
      throw TabaError.client('client.invalid-request', 'update is required');
    }
    const c = withJson(call('PATCH', `/topics/${segment(name)}`), updateJson(update));
    const reply = await this._send(c, signal);
    return parseTopic(reply.text);
  }

  async deleteTopic(name, { signal } = {}) {
    const c = call('DELETE', `/topics/${segment(name)}`);
    c.recover.push([404, 'topic.not-found']);
    await this._send(c, signal);
  }

  /**
   * `POST /topics/{topic}/messages`. The body goes as bytes (a `Buffer`, `Uint8Array` or string), without
   * validation. Options: `contentType` (default `application/json`), `groupId`, `deduplicationId`,
   * `requestId`, `traceparent`, `fifo` (set it for a FIFO queue: only there can a 502/504 be repeated) and `signal`.
   */
  async publish(topic, body, options) {
    const o = options || {};
    const c = call('POST', `/topics/${segment(topic)}/messages`);
    c.body = body === undefined || body === null ? Buffer.alloc(0) : Buffer.from(body);
    c.contentType = o.contentType || 'application/json';
    c.requestId = o.requestId;
    c.traceparent = o.traceparent;
    c.retryGateway = Boolean(o.fifo);
    if (o.groupId !== undefined) withQuery(c, 'groupId', o.groupId);
    if (o.deduplicationId !== undefined) withQuery(c, 'deduplicationId', o.deduplicationId);
    const reply = await this._send(c, o.signal);
    return parsePublishResponse(reply.text);
  }

  /** Serializes the value (with the configured `serializer`, or `JSON`) and publishes as `application/json`. */
  publishJson(topic, value, options) {
    if (value === undefined || value === null) {
      throw TabaError.client('client.invalid-request', 'value is required');
    }
    let text;
    try {
      text = this._serializer.stringify(value);
    } catch (e) {
      throw TabaError.client('client.invalid-request', `cannot serialize the value: ${e.message}`, e);
    }
    return this.publish(topic, Buffer.from(text, 'utf8'), Object.assign({}, options, { contentType: 'application/json' }));
  }

  /** Publishes a JSON text the application already built, without serializing. */
  publishRawJson(topic, json, options) {
    return this.publish(topic, Buffer.from(json, 'utf8'), Object.assign({}, options, { contentType: 'application/json' }));
  }

  /** Publishes a text (`Content-Type: text/plain; charset=utf-8`). */
  publishText(topic, text, options) {
    return this.publish(topic, Buffer.from(text, 'utf8'), Object.assign({}, options, { contentType: 'text/plain; charset=utf-8' }));
  }

  /**
   * `GET /topics/{topic}/messages`. The read time is `waitTime + 10 s`, not the `requestTimeout`. A timeout is
   * not repeated (the node may have captured messages, which come back by themselves at the end of the
   * `visibilityTimeout`). Resolves an empty array when there are no messages.
   */
  async consume(topic, options) {
    const reply = await this._send(consumeCall(topic, options), options && options.signal);
    return parseMessages(reply.text);
  }

  /** `DELETE /topics/{topic}/messages/{id}?receiptToken=`. `message` is what `consume` returned. */
  async ack(topic, message, { signal } = {}) {
    const c = call('DELETE', `/topics/${segment(topic)}/messages/${segment(message.id)}`);
    withQuery(c, 'receiptToken', message.receiptToken);
    await this._send(c, signal);
  }

  /** `PUT /topics/{topic}/messages/{id}/visibility?receiptToken=`. */
  async changeVisibility(topic, message, seconds, { signal } = {}) {
    const c = call('PUT', `/topics/${segment(topic)}/messages/${segment(message.id)}/visibility`);
    withQuery(c, 'receiptToken', message.receiptToken);
    withJson(c, JSON.stringify({ visibilityTimeout: seconds }));
    const reply = await this._send(c, signal);
    return parseVisibilityResponse(reply.text);
  }

  /**
   * Reads from the queue and hands each message to `handler(context)`. The message is deleted if the handler
   * finishes without throwing (`context.delete = false` keeps it). While the handler runs, the invisibility is
   * renewed every third of it. Defaults: one message at a time, 20 s of long polling, 120 s of invisibility.
   * If the handler throws, the message is not deleted and the error propagates. Resolves how many messages
   * were handled.
   */
  async consumeWith(topic, options, handler) {
    if (typeof handler !== 'function') {
      throw TabaError.client('client.invalid-request', 'handler is required');
    }
    const o = handlerDefaults(options);
    const messages = await this.consume(topic, o);
    let handled = 0;
    for (const message of messages) {
      const context = this._context(topic, o, message);
      await this._handle(topic, context, () => handler(context), o.signal);
      handled++;
    }
    return handled;
  }

  /**
   * `consumeWith` with the body already parsed (with the configured `serializer`, or `JSON`) in
   * `context.body`. A body that does not parse gives `client.invalid-response` before the handler is
   * called, and the message is not deleted.
   */
  async consumeJson(topic, options, handler) {
    if (typeof handler !== 'function') {
      throw TabaError.client('client.invalid-request', 'handler is required');
    }
    const o = handlerDefaults(options);
    const messages = await this.consume(topic, o);
    let handled = 0;
    for (const message of messages) {
      let body;
      try {
        body = this._serializer.parse(message.bodyText);
      } catch (e) {
        throw TabaError.client('client.invalid-response', `body of message ${message.id} is not valid JSON: ${e.message}`, e);
      }
      const context = this._context(topic, o, message);
      context.body = body;
      await this._handle(topic, context, () => handler(context), o.signal);
      handled++;
    }
    return handled;
  }

  _context(topic, options, message) {
    return new MessageContext(
      message,
      options.visibilityTimeout,
      (seconds) => this.changeVisibility(topic, message, seconds, { signal: options.signal }),
      options.signal,
    );
  }

  async _handle(topic, context, run, signal) {
    const stop = new AbortController();
    const onAbort = () => stop.abort();
    if (signal) {
      signal.addEventListener('abort', onAbort, { once: true });
    }
    const renewal = this._keepAlive(topic, context, stop.signal);
    try {
      await run();
    } finally {
      stop.abort();
      if (signal) {
        signal.removeEventListener('abort', onAbort);
      }
      await renewal;
    }
    if (context.delete) {
      try {
        // Without the signal: the message was already handled, and acknowledging it must not be cut short.
        await this.ack(topic, context.message);
      } catch (e) {
        if (!(e instanceof TabaError && (e.status === 409 || e.status === 404))) {
          throw e;
        }
        // The message already went back to the queue (deadline expired) or is already gone.
      }
    }
  }

  /** The second task: renews the invisibility until the handler finishes. Never rejects. */
  async _keepAlive(topic, context, signal) {
    try {
      while (!signal.aborted) {
        await this._transport.sleep(renewalInterval(context.leaseSeconds), signal);
        try {
          await this.changeVisibility(topic, context.message, context.leaseSeconds, { signal });
        } catch (e) {
          if (e instanceof TabaError && (e.status === 409 || e.status === 404)) {
            // The message already went back to the queue or was deleted: renewing is no longer of any use.
            context._lost = true;
            return;
          }
          if (signal.aborted) {
            return;
          }
          // Transient failure (network, 5xx): tries again on the next round, before the deadline expires.
        }
      }
    } catch (e) {
      // The handler finished (the signal was aborted while waiting).
    }
  }

  /**
   * The consume loop: long polling (`waitTime` 20 and one message at a time, if not set), calls
   * `handler(message)` and acknowledges by itself only if it finishes without throwing (a message that fails
   * goes back to the queue at the end of the `visibilityTimeout`, and the error goes to `onError(message, error)`).
   * Resolves, without an error, when `signal` is aborted. A server error while consuming rejects.
   */
  async subscribe(topic, options, handler, { signal, onError } = {}) {
    if (typeof handler !== 'function') {
      throw TabaError.client('client.invalid-request', 'handler is required');
    }
    const poll = Object.assign({}, options, { signal });
    if (poll.waitTime === undefined) poll.waitTime = HANDLER_WAIT_TIME;
    if (poll.maxMessages === undefined) poll.maxMessages = HANDLER_MAX_MESSAGES;
    while (!(signal && signal.aborted)) {
      let messages;
      try {
        messages = await this.consume(topic, poll);
      } catch (e) {
        if (isAbort(e, signal)) {
          return;
        }
        throw e;
      }
      for (const message of messages) {
        let handled;
        try {
          await handler(message);
          handled = true;
        } catch (e) {
          handled = false;
          if (onError) {
            onError(message, e);
          }
        }
        if (handled) {
          try {
            // Without the signal: the message was already handled, and acknowledging it must not be cut short.
            await this.ack(topic, message);
          } catch (e) {
            if (!(e instanceof TabaError && (e.status === 409 || e.status === 404))) {
              throw e;
            }
          }
        }
        if (signal && signal.aborted) {
          return;
        }
      }
    }
  }
}

module.exports = { TabaClient, VERSION, CONTRACT_VERSION, consumeCall, renewalInterval };
