'use strict';

const { TabaError } = require('./errors');
const { JsonError, JsonFields, arrayItems, decodeString } = require('./json');

/** A delivered message. The `body` always comes out as bytes (doc 21, "The delivered message"). */
class Message {
  constructor(fields) {
    Object.assign(this, fields);
  }

  /** The `body` as UTF-8 text. */
  get bodyText() {
    return this.body.toString('utf8');
  }

  /** The `body` read as JSON. */
  json() {
    return JSON.parse(this.bodyText);
  }
}

/**
 * What the `consumeWith` handler receives: the `message`, the `delete` flag (the message is deleted at the end
 * if the handler finishes without throwing; the default is `true`) and `changeVisibility`. While the handler
 * runs, the connector renews the invisibility by itself, so no other consumer takes the message.
 */
class MessageContext {
  constructor(message, leaseSeconds, change, signal) {
    this.message = message;
    this.delete = true;
    this.signal = signal;
    this._leaseSeconds = leaseSeconds;
    this._lost = false;
    this._change = change;
  }

  /** The seconds of invisibility the automatic renewal requests on each round. */
  get leaseSeconds() {
    return this._leaseSeconds;
  }

  /** `true` when the server refused the renewal (the message already went back to the queue or was deleted). */
  get leaseLost() {
    return this._lost;
  }

  /** Changes the invisibility to `seconds` from now on. The value becomes the one of the automatic renewal. */
  async changeVisibility(seconds) {
    await this._change(seconds);
    this._leaseSeconds = seconds;
  }
}

function invalid(what, e) {
  if (e instanceof JsonError) {
    return TabaError.client('client.invalid-response', `${what}: ${e.message}`);
  }
  return e;
}

function fields(raw, what) {
  try {
    return JsonFields.parse(raw);
  } catch (e) {
    throw invalid(what, e);
  }
}

function isJsonType(contentType) {
  const media = contentType.split(';')[0].trim().toLowerCase();
  return media === 'application/json' || media.endsWith('+json');
}

function parseTopic(raw) {
  const f = fields(raw, 'topic');
  try {
    return {
      id: f.string('id') || '',
      name: f.string('name') || '',
      type: f.string('type') || '',
      createdAt: f.string('createdAt') || '',
      fifo: f.boolean('fifo'),
      volatile: f.boolean('volatile'),
      raw,
    };
  } catch (e) {
    throw invalid('topic', e);
  }
}

function parseMessage(raw) {
  const f = fields(raw, 'message');
  try {
    const contentType = f.string('contentType') || 'application/json';
    const body = f.raw('body');
    let bytes;
    if (body === null) {
      bytes = Buffer.alloc(0);
    } else if (f.string('bodyEncoding') === 'base64') {
      const text = decodeString(body);
      if (!/^[A-Za-z0-9+/]*={0,2}$/.test(text) || text.length % 4 !== 0) {
        throw TabaError.client('client.invalid-response', 'invalid base64 body');
      }
      bytes = Buffer.from(text, 'base64');
    } else if (isJsonType(contentType)) {
      bytes = Buffer.from(body, 'utf8');
    } else {
      bytes = Buffer.from(decodeString(body), 'utf8');
    }
    return new Message({
      id: f.string('id') || '',
      body: bytes,
      contentType,
      receiptToken: f.string('receiptToken') || '',
      deliveryCount: f.integer('deliveryCount') || 0,
      publishedAt: f.string('publishedAt') || '',
      invisibleUntil: f.string('invisibleUntil') || '',
      groupId: f.string('groupId'),
      deduplicationId: f.string('deduplicationId'),
      deadLetteredAt: f.string('deadLetteredAt'),
      deadLetterReason: f.string('deadLetterReason'),
      raw,
    });
  } catch (e) {
    throw invalid('message', e);
  }
}

function parseMessages(raw) {
  const f = fields(raw, 'consume');
  try {
    return arrayItems(f.raw('messages') || '[]').map(parseMessage);
  } catch (e) {
    throw invalid('messages', e);
  }
}

function parsePage(raw) {
  const f = fields(raw, 'page');
  try {
    return {
      items: arrayItems(f.raw('items') || '[]').map(parseTopic),
      nextCursor: f.string('nextCursor'),
      raw,
    };
  } catch (e) {
    throw invalid('page items', e);
  }
}

function parsePublishResponse(raw) {
  const f = fields(raw, 'publication');
  try {
    const deliveries = [];
    const list = f.raw('deliveries');
    if (list !== null) {
      for (const item of arrayItems(list)) {
        const d = JsonFields.parse(item);
        const error = d.raw('error');
        const e = error !== null && error.startsWith('{') ? JsonFields.parse(error) : null;
        deliveries.push({
          topic: d.string('topic') || '',
          id: d.string('id'),
          errorCode: e === null ? null : e.string('code') || '',
          errorMessage: e === null ? null : e.string('message'),
          succeeded: e === null,
        });
      }
    }
    return {
      id: f.string('id'),
      duplicate: f.boolean('duplicate'),
      matched: f.integer('matched'),
      deliveries,
      raw,
    };
  } catch (e) {
    throw invalid('publication', e);
  }
}

function parseVisibilityResponse(raw) {
  const f = fields(raw, 'visibility');
  try {
    return { id: f.string('id') || '', invisibleUntil: f.string('invisibleUntil') || '', raw };
  } catch (e) {
    throw invalid('visibility', e);
  }
}

module.exports = {
  Message,
  MessageContext,
  parseTopic,
  parseMessage,
  parseMessages,
  parsePage,
  parsePublishResponse,
  parseVisibilityResponse,
};
