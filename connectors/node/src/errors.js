'use strict';

/**
 * The only kind of error of the connector (doc 21, "The common contract"): the API `ErrorResponse`, or a
 * failure of the client itself, with `status` zero and `code` `client.network`, `client.timeout`,
 * `client.invalid-config`, `client.invalid-response` or `client.invalid-request`. The application tests the
 * `code`, never the `message`.
 */
class TabaError extends Error {
  /**
   * @param {number} status HTTP status, or zero on a failure of the client itself.
   * @param {string} code
   * @param {string} message
   * @param {{requestId?: string, retryAfter?: number, details?: string, body?: Buffer, cause?: unknown}} [extra]
   */
  constructor(status, code, message, extra) {
    super(message, extra && extra.cause !== undefined ? { cause: extra.cause } : undefined);
    this.name = 'TabaError';
    this.status = status;
    this.code = code || '';
    this.requestId = extra && extra.requestId !== undefined ? extra.requestId : undefined;
    this.retryAfter = extra && extra.retryAfter !== undefined ? extra.retryAfter : undefined;
    this.details = extra && extra.details !== undefined ? extra.details : undefined;
    this.body = extra && extra.body ? extra.body : Buffer.alloc(0);
  }

  /** A failure of the client itself, not a server response. */
  get isClientSide() {
    return this.status === 0;
  }

  static client(code, message, cause) {
    return new TabaError(0, code, message, { cause });
  }
}

module.exports = { TabaError };
