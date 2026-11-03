/// <reference types="node" />

export const VERSION: string;
export const CONTRACT_VERSION: string;

export interface TabaConfig {
  /** Address of the client API load balancer (`http://host:port` or `https://...`). Required. */
  baseUrl: string;
  /** Goes in `Authorization: Bearer`. */
  token?: string;
  /** Milliseconds to open the connection (5000). */
  connectTimeout?: number;
  /** Milliseconds of an ordinary call (30000). `consume` uses `waitTime + 10 s`. */
  requestTimeout?: number;
  /** Attempts, counting the first (3). */
  maxAttempts?: number;
  /** Goes in `User-Agent` (`taba-node/<version>`). */
  userAgent?: string;
  /** Who serializes `publishJson` and parses `consumeJson`. Default: `JSON`. */
  serializer?: { stringify(value: unknown): string; parse(text: string): unknown };
  /** An own `http.Agent` / `https.Agent` (a proxy, for example). The client does not destroy it. */
  agent?: import('node:http').Agent;
}

export interface CallOptions {
  signal?: AbortSignal;
}

export interface Topic {
  id: string;
  name: string;
  /** `queue` or `distributor`. */
  type: string;
  createdAt: string;
  fifo: boolean | null;
  volatile: boolean | null;
  /** The topic JSON, as the server returned it. */
  raw: string;
}

export interface Page<T> {
  items: T[];
  nextCursor: string | null;
  raw: string;
}

export interface TopicRequest {
  name: string;
  type: 'queue' | 'distributor' | string;
  tags?: Record<string, string>;
  prefixes?: string[];
  volatile?: boolean;
  idleTimeout?: number;
  fifo?: boolean;
  maxMessageBytes?: number;
  ttl?: number;
  maxDeliveries?: number;
}

/** `undefined` leaves the field out; `null` (where allowed) goes back to the server default. */
export interface TopicUpdate {
  tags?: Record<string, string>;
  prefixes?: string[];
  idleTimeout?: number;
  maxMessageBytes?: number | null;
  ttl?: number | null;
  maxDeliveries?: number | null;
}

export interface PublishOptions extends CallOptions {
  /** Default `application/json`. */
  contentType?: string;
  groupId?: string;
  /** FIFO queue only. */
  deduplicationId?: string;
  requestId?: string;
  /** `traceparent` header (W3C Trace Context). */
  traceparent?: string;
  /** Set it when the topic is a FIFO queue: only there can a 502/504 be repeated. */
  fifo?: boolean;
}

export interface DistributorDelivery {
  topic: string;
  id: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  succeeded: boolean;
}

export interface PublishResponse {
  /** The message id (queue). `null` in a distributor's response. */
  id: string | null;
  /** FIFO queue only. */
  duplicate: boolean | null;
  /** Distributor only: how many queues matched. */
  matched: number | null;
  deliveries: DistributorDelivery[];
  raw: string;
}

export interface ConsumeOptions extends CallOptions {
  maxMessages?: number;
  /** Seconds of invisibility for the delivered messages. */
  visibilityTimeout?: number;
  /** Seconds of long polling. */
  waitTime?: number;
  autoCreate?: boolean;
  /** The next four only apply with `autoCreate`. */
  volatile?: boolean;
  idleTimeout?: number;
  fifo?: boolean;
  tags?: Record<string, string>;
  requestId?: string;
  traceparent?: string;
}

export interface MessageRef {
  id: string;
  receiptToken: string;
}

export class Message implements MessageRef {
  id: string;
  /** JSON: the JSON text as it came. `text/*`: the UTF-8 text. base64: the bytes already decoded. */
  body: Buffer;
  contentType: string;
  receiptToken: string;
  deliveryCount: number;
  publishedAt: string;
  invisibleUntil: string;
  groupId: string | null;
  deduplicationId: string | null;
  deadLetteredAt: string | null;
  deadLetterReason: string | null;
  /** The message JSON, as the server returned it. */
  raw: string;
  /** The `body` as UTF-8 text. */
  readonly bodyText: string;
  /** The `body` read as JSON. */
  json(): unknown;
}

export class MessageContext<T = undefined> {
  readonly message: Message;
  /** `consumeJson` only: the parsed body. */
  body: T;
  /** `true` (the default): the message is deleted when the handler finishes without throwing. */
  delete: boolean;
  readonly signal: AbortSignal | undefined;
  readonly leaseSeconds: number;
  /** `true` when the server refused the renewal (the message already went back to the queue or was deleted). */
  readonly leaseLost: boolean;
  changeVisibility(seconds: number): Promise<void>;
}

export interface VisibilityResponse {
  id: string;
  invisibleUntil: string;
  raw: string;
}

/** One message of `publishBatch`: the same fields as `publish`. The bytes travel in base64. */
export interface BatchMessage {
  body: Buffer | Uint8Array | string;
  /** Default `application/json`. */
  contentType?: string;
  groupId?: string;
  deduplicationId?: string;
}

/** The result of one item of a batch, in the order of the request. An item that failed does not reject. */
export interface BatchResult {
  /** `201` published, `200` a FIFO duplicate, a distributor or a new invisibility, `204` acked, or the error's. */
  status: number;
  /** The item was applied. */
  ok: boolean;
  id: string | null;
  duplicate: boolean | null;
  matched: number | null;
  invisibleUntil: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  raw: string;
}

/**
 * The only kind of error: the API `ErrorResponse`, or a failure of the client itself (`status` 0 and
 * `code` `client.network`, `client.timeout`, `client.invalid-config`, `client.invalid-response` or
 * `client.invalid-request`). Test the `code`, never the `message`.
 */
export class TabaError extends Error {
  /** The HTTP status, or zero on a failure of the client itself. */
  readonly status: number;
  readonly code: string;
  readonly requestId: string | undefined;
  /** Seconds of the `Retry-After`. */
  readonly retryAfter: number | undefined;
  /** The `details` of the `ErrorResponse`, as JSON text. */
  readonly details: string | undefined;
  readonly body: Buffer;
  readonly isClientSide: boolean;
  constructor(status: number, code: string, message: string, extra?: object);
}

export interface SubscribeOptions extends CallOptions {
  onError?: (message: Message, error: unknown) => void;
}

export class TabaClient {
  constructor(config: TabaConfig);
  close(): void;

  createTopic(request: TopicRequest, options?: CallOptions): Promise<Topic>;
  getTopic(name: string, options?: CallOptions): Promise<Topic>;
  listTopics(options?: { limit?: number; cursor?: string | null; name?: string } & CallOptions): Promise<Page<Topic>>;
  listAllTopics(options?: { name?: string } & CallOptions): Promise<Topic[]>;
  updateTopic(name: string, update: TopicUpdate, options?: CallOptions): Promise<Topic>;
  deleteTopic(name: string, options?: CallOptions): Promise<void>;

  publish(topic: string, body: Buffer | Uint8Array | string, options?: PublishOptions): Promise<PublishResponse>;
  publishJson(topic: string, value: unknown, options?: PublishOptions): Promise<PublishResponse>;
  publishRawJson(topic: string, json: string, options?: PublishOptions): Promise<PublishResponse>;
  publishText(topic: string, text: string, options?: PublishOptions): Promise<PublishResponse>;
  publishBatch(topic: string, messages: BatchMessage[], options?: CallOptions): Promise<BatchResult[]>;

  consume(topic: string, options?: ConsumeOptions): Promise<Message[]>;
  consumeWith(
    topic: string,
    options: ConsumeOptions | undefined,
    handler: (context: MessageContext) => Promise<void> | void,
  ): Promise<number>;
  consumeJson<T = unknown>(
    topic: string,
    options: ConsumeOptions | undefined,
    handler: (context: MessageContext<T>) => Promise<void> | void,
  ): Promise<number>;

  ack(topic: string, message: MessageRef, options?: CallOptions): Promise<void>;
  changeVisibility(topic: string, message: MessageRef, seconds: number, options?: CallOptions): Promise<VisibilityResponse>;
  ackBatch(topic: string, messages: MessageRef[], options?: CallOptions): Promise<BatchResult[]>;
  changeVisibilityBatch(topic: string, messages: MessageRef[], seconds: number, options?: CallOptions): Promise<BatchResult[]>;

  subscribe(
    topic: string,
    options: ConsumeOptions | undefined,
    handler: (message: Message) => Promise<void> | void,
    subscribeOptions?: SubscribeOptions,
  ): Promise<void>;
}
