'use strict';

const http = require('node:http');
const https = require('node:https');
const { TabaError } = require('./errors');
const { JsonFields } = require('./json');

// Refused, DNS and unreachable: the body was never sent, so the call can be repeated like a 503.
const CONNECT_FAILURES = new Set(['ECONNREFUSED', 'ENOTFOUND', 'EAI_AGAIN', 'ENETUNREACH', 'EHOSTUNREACH']);

const connectFailures = new WeakSet();

function sleep(ms, signal) {
  return new Promise((resolve, reject) => {
    if (signal && signal.aborted) {
      reject(signal.reason);
      return;
    }
    const onAbort = () => {
      clearTimeout(timer);
      reject(signal.reason);
    };
    const timer = setTimeout(() => {
      if (signal) {
        signal.removeEventListener('abort', onAbort);
      }
      resolve();
    }, ms);
    if (signal) {
      signal.addEventListener('abort', onAbort, { once: true });
    }
  });
}

function validateBaseUrl(baseUrl) {
  let url = null;
  if (typeof baseUrl === 'string' && baseUrl.trim() !== '') {
    try {
      url = new URL(baseUrl.trim());
    } catch (e) {
      url = null;
    }
  }
  if (url === null || (url.protocol !== 'http:' && url.protocol !== 'https:')) {
    const shown = typeof baseUrl === 'string' && baseUrl.trim() !== '' ? baseUrl : 'empty';
    throw TabaError.client(
      'client.invalid-config',
      `baseUrl must be an http:// or https:// address (received: ${shown})`,
    );
  }
  return url;
}

function errorFrom(status, headers, body) {
  const text = body.toString('utf8');
  let code = null;
  let message = null;
  let requestId = null;
  let details = null;
  try {
    const f = JsonFields.parse(text);
    code = f.string('code');
    if (code !== null) {
      message = f.string('message') || '';
      requestId = f.string('requestId');
      details = f.raw('details');
    }
  } catch (e) {
    // Not an ErrorResponse: falls to the generic code below.
  }
  if (code === null) {
    code = `http.${status}`;
    const trimmed = text.trim();
    message = trimmed.length === 0 ? `response ${status} with no body` : trimmed.slice(0, 300);
  }
  const retryHeader = headers['retry-after'];
  const retryAfter = typeof retryHeader === 'string' && /^\d+$/.test(retryHeader.trim())
    ? Number(retryHeader.trim())
    : undefined;
  return new TabaError(status, code, message, {
    requestId: requestId || headers['x-request-id'] || undefined,
    retryAfter,
    details: details === null ? undefined : details,
    body,
  });
}

function errorCodeOf(body) {
  try {
    return JsonFields.parse(body.toString('utf8')).string('code');
  } catch (e) {
    return null;
  }
}

function recovers(call, status, body) {
  return call.recover.some(([s, code]) => s === status && errorCodeOf(body) === code);
}

function describe(e) {
  const code = e.code ? `${e.code}: ` : '';
  return code + (e.message || String(e));
}

/** Percent-encodes for a path segment or a query part (RFC 3986 unreserved characters stay). */
function encode(value) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
}

/**
 * The client's HTTP transport: builds the request, sends it, reads the body, turns the API error into a
 * `TabaError` and does the call retries of the common contract (doc 21).
 */
class Transport {
  constructor(config, sleepFn) {
    this._config = config;
    this._url = validateBaseUrl(config.baseUrl);
    this._base = config.baseUrl.trim().replace(/\/+$/, '');
    this._lib = this._url.protocol === 'https:' ? https : http;
    this._owned = !config.agent;
    const agentOptions = { keepAlive: true };
    if (this._url.protocol === 'https:') {
      agentOptions.minVersion = 'TLSv1.2';
    }
    this._agent = config.agent || new this._lib.Agent(agentOptions);
    this.sleep = sleepFn || sleep;
  }

  close() {
    if (this._owned) {
      this._agent.destroy();
    }
  }

  _path(call) {
    let path = new URL(this._base + call.path).pathname;
    if (call.query.length > 0) {
      path += '?' + call.query.map(([k, v]) => `${encode(k)}=${encode(v)}`).join('&');
    }
    return path;
  }

  _headers(call) {
    const headers = { Accept: 'application/json', 'User-Agent': this._config.userAgent };
    if (this._config.token) {
      headers.Authorization = `Bearer ${this._config.token}`;
    }
    if (call.requestId) {
      headers['X-Request-Id'] = call.requestId;
    }
    if (call.traceparent) {
      headers.traceparent = call.traceparent;
    }
    if (call.body) {
      headers['Content-Type'] = call.contentType || 'application/json';
      headers['Content-Length'] = call.body.length;
    }
    return headers;
  }

  /** One attempt: resolves `{status, headers, body}`; rejects with a `TabaError` or the abort reason. */
  _once(call, timeoutMs, signal) {
    return new Promise((resolve, reject) => {
      if (signal && signal.aborted) {
        reject(signal.reason);
        return;
      }
      let settled = false;
      let timedOut = false;
      let gotResponse = false;
      let connectTimer = null;
      const onAbort = () => {
        req.destroy();
        finish(reject, signal.reason);
      };
      const finish = (fn, value) => {
        if (settled) {
          return;
        }
        settled = true;
        clearTimeout(timer);
        clearTimeout(connectTimer);
        if (signal) {
          signal.removeEventListener('abort', onAbort);
        }
        fn(value);
      };
      const req = this._lib.request({
        method: call.method,
        hostname: this._url.hostname.replace(/^\[|\]$/g, ''),
        port: this._url.port || undefined,
        path: this._path(call),
        headers: this._headers(call),
        agent: this._agent,
      }, (res) => {
        gotResponse = true;
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => finish(resolve, { status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
        res.on('error', (e) => finish(reject, TabaError.client('client.network', describe(e), e)));
      });
      const timer = setTimeout(() => {
        timedOut = true;
        req.destroy();
        finish(reject, TabaError.client('client.timeout', 'timed out waiting for the response'));
      }, timeoutMs);
      if (signal) {
        signal.addEventListener('abort', onAbort, { once: true });
      }
      req.on('socket', (socket) => {
        if (socket.connecting) {
          connectTimer = setTimeout(() => {
            req.destroy(Object.assign(new Error('timed out connecting'), { code: 'ETIMEDOUT' }));
          }, this._config.connectTimeout);
          socket.once('connect', () => clearTimeout(connectTimer));
        }
      });
      req.on('error', (e) => {
        if (timedOut) {
          return;
        }
        const error = TabaError.client('client.network', describe(e), e);
        // A pooled connection the server closed while idle fails before anything is sent.
        const stale = req.reusedSocket && !gotResponse && (e.code === 'ECONNRESET' || e.code === 'EPIPE');
        if (CONNECT_FAILURES.has(e.code) || stale) {
          connectFailures.add(error);
        }
        finish(reject, error);
      });
      req.end(call.body || undefined);
    });
  }

  /**
   * Makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the
   * `Retry-After`, or 1 s, 2 s, 4 s), 502 and 504 only with `retryGateway`, and a failure to connect
   * (refused, DNS). Never a 4xx (except 429), nor a failure after the body was sent, nor a timeout.
   */
  async send(call, signal) {
    const timeoutMs = call.timeout !== undefined ? call.timeout : this._config.requestTimeout;
    const max = Math.max(1, this._config.maxAttempts);
    let afterRetryable = false;
    for (let attempt = 1; ; attempt++) {
      const backoff = 1000 * 2 ** Math.min(attempt - 1, 6);
      let wait;
      try {
        const { status, headers, body } = await this._once(call, timeoutMs, signal);
        if (status < 400) {
          return { status, body, text: body.toString('utf8'), recovered: false };
        }
        if (afterRetryable && recovers(call, status, body)) {
          return { status, body, text: body.toString('utf8'), recovered: true };
        }
        const error = errorFrom(status, headers, body);
        const retry = status === 429 || status === 503 || ((status === 502 || status === 504) && call.retryGateway);
        if (!retry || attempt >= max) {
          throw error;
        }
        afterRetryable = afterRetryable || status === 503;
        wait = error.retryAfter !== undefined ? error.retryAfter * 1000 : backoff;
      } catch (e) {
        if (!connectFailures.has(e) || attempt >= max) {
          throw e;
        }
        afterRetryable = true;
        wait = backoff;
      }
      await this.sleep(wait, signal);
    }
  }
}

module.exports = { Transport, sleep, encode };
