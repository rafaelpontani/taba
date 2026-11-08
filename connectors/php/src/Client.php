<?php

namespace Taba;

/**
 * The Taba client API client (doc 21): the nine operations, `subscribe` and the consume with a handler.
 *
 * PHP runs one thread: use one client per process (it keeps one connection open and reuses it).
 *
 *     $client = new Taba\Client(new Taba\Config('http://localhost:8080', $token));
 *     $client->publishJson('orders', array('order' => 9001));
 *     $client->consumeWith('orders', function (Taba\MessageContext $context) {
 *         echo $context->message->bodyText();   // deleted when the handler returns without throwing
 *     }, array('waitTime' => 20));
 */
class Client
{
    const VERSION = '0.1.0';
    const CONTRACT_VERSION = '1';

    private $transport;

    /**
     * @param Config|string|array $config  a Config, the address, or an array of the Config fields
     * @param callable|null       $sleep    test hook: `function ($seconds, CancelToken $cancel)`
     * @param callable|null       $executor test hook: replaces the `curl` call
     */
    public function __construct($config, $sleep = null, $executor = null)
    {
        $this->transport = new Transport(Config::from($config), $sleep, $executor);
    }

    public function config()
    {
        return $this->transport->config();
    }

    /** Closes the connection. The client can be used again afterwards. */
    public function close()
    {
        $this->transport->close();
    }

    private function send(ApiCall $call, $cancel = null)
    {
        return $this->transport->send($call, $cancel);
    }

    // -- topics ---------------------------------------------------------------------------------------------

    /** `POST /topics`. `$request` is a TopicRequest or an array of its fields. */
    public function createTopic($request, $cancel = null)
    {
        $request = TopicRequest::from($request);
        $reply = $this->send(Calls::createTopic($request), $cancel);
        if ($reply->recovered) {
            return $this->getTopic($request->name, $cancel);
        }
        return Parser::topic($reply->body);
    }

    /** `GET /topics/{topic}`. */
    public function getTopic($name, $cancel = null)
    {
        return Parser::topic($this->send(Calls::getTopic($name), $cancel)->body);
    }

    /** `GET /topics`. Parameters left null do not go in the query. */
    public function listTopics($limit = null, $cursor = null, $name = null, $cancel = null)
    {
        return Parser::page($this->send(Calls::listTopics($limit, $cursor, $name), $cancel)->body);
    }

    /** All topics, page by page. */
    public function listAllTopics($name = null, $cancel = null)
    {
        $all = array();
        $cursor = null;
        do {
            $page = $this->listTopics(null, $cursor, $name, $cancel);
            $all = array_merge($all, $page->items);
            $cursor = $page->nextCursor;
        } while ($cursor !== null);
        return $all;
    }

    /** `PATCH /topics/{topic}`: a real `PATCH`, with no verb-override header. */
    public function updateTopic($name, $update, $cancel = null)
    {
        return Parser::topic($this->send(Calls::updateTopic($name, $update), $cancel)->body);
    }

    /** `DELETE /topics/{topic}`. */
    public function deleteTopic($name, $cancel = null)
    {
        $this->send(Calls::deleteTopic($name), $cancel);
    }

    /**
     * `PUT /topics/{topic}`: brings the topic to the declared state, creating it if it does not exist. What is left
     * out goes back to the server default (a full replacement, unlike `updateTopic`); the name of the request is the
     * topic of the path. Another `type`, `fifo` or `volatile` is `409 topic.immutable-mismatch` unless
     * `$recreate` (`'if-empty'` or `'always'`) says otherwise. Repeated after a 503 like the other calls: the route
     * is idempotent. `$request` is a TopicRequest or an array of its fields.
     *
     * @return TopicPut
     */
    public function createOrUpdateTopic($request, $recreate = null, $cancel = null)
    {
        $reply = $this->send(Calls::putTopic($request, $recreate), $cancel);
        if ($reply->status === 201) {
            $outcome = $reply->recreated ? TopicPut::RECREATED : TopicPut::CREATED;
        } else {
            $outcome = TopicPut::UPDATED;
        }
        return new TopicPut(Parser::topic($reply->body), $outcome);
    }

    /**
     * `DELETE /topics/{topic}?ifEmpty=true`: removes the queue only if it and its dead-letter hold no message on any
     * node (`409 topic.not-empty` otherwise; a distributor holds none).
     */
    public function deleteTopicIfEmpty($name, $cancel = null)
    {
        $this->send(Calls::deleteTopic($name, true), $cancel);
    }

    // -- publishing -----------------------------------------------------------------------------------------

    /**
     * `POST /topics/{topic}/messages`. The body goes as bytes (a PHP string), without validation.
     * Set `fifo` in the options when the topic is a FIFO queue: only there can a 502/504 be repeated.
     *
     * @param PublishOptions|array|null $options
     */
    public function publish($topic, $body, $options = null, $cancel = null)
    {
        $reply = $this->send(Calls::publish($topic, $body, PublishOptions::from($options)), $cancel);
        return Parser::publishResponse($reply->body);
    }

    /** Serializes the value with `json_encode` and publishes it as `application/json`. */
    public function publishJson($topic, $value, $options = null, $cancel = null)
    {
        if ($value === null) {
            throw TabaException::client('client.invalid-request', 'value is required');
        }
        $text = json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        if ($text === false) {
            throw TabaException::client('client.invalid-request', 'cannot serialize the value: ' . Calls::jsonMessage());
        }
        return $this->publishAs($topic, $text, 'application/json', $options, $cancel);
    }

    /** Publishes a JSON text the application already built, without serializing. */
    public function publishRawJson($topic, $json, $options = null, $cancel = null)
    {
        return $this->publishAs($topic, $json, 'application/json', $options, $cancel);
    }

    /** Publishes a text (`Content-Type: text/plain; charset=utf-8`). */
    public function publishText($topic, $text, $options = null, $cancel = null)
    {
        return $this->publishAs($topic, $text, 'text/plain; charset=utf-8', $options, $cancel);
    }

    private function publishAs($topic, $body, $contentType, $options, $cancel)
    {
        $o = PublishOptions::from($options);
        $o->contentType = $contentType;
        return $this->publish($topic, $body, $o, $cancel);
    }

    /**
     * `POST /topics/{topic}/messages/batch`: publishes many messages in one request (1 to the server's
     * `api.batch.max-messages`, 1000 by default). Each item is answered as its single publication and one that
     * fails does not undo the others (look at `ok()`). On a FIFO queue the items of a group are published in the
     * order of the array. A failure of the network is not repeated (it could duplicate).
     *
     * @param BatchMessage[] $messages
     * @return BatchResult[]
     */
    public function publishBatch($topic, array $messages, $cancel = null)
    {
        return Parser::batchResults($this->send(Calls::publishBatch($topic, $messages), $cancel)->body);
    }

    // -- consuming ------------------------------------------------------------------------------------------

    /**
     * `GET /topics/{topic}/messages`. The read time is `waitTime + 10` seconds, not the `requestTimeout`. A
     * timeout is not repeated (the node may have captured messages, which come back by themselves at the end of
     * the `visibilityTimeout`). Returns an empty array if there are no messages.
     *
     * @param ConsumeOptions|array|null $options
     * @return Message[]
     */
    public function consume($topic, $options = null, $cancel = null)
    {
        return $this->fetch($topic, ConsumeOptions::from($options), $cancel);
    }

    private function fetch($topic, ConsumeOptions $options, $cancel)
    {
        return Parser::messages($this->send(Calls::consume($topic, $options), $cancel)->body);
    }

    private static function reference($message, $receiptToken)
    {
        if ($message instanceof Message) {
            return array($message->id, $message->receiptToken);
        }
        if ($receiptToken === null) {
            throw TabaException::client('client.invalid-request', 'receiptToken is required with a message id');
        }
        return array((string) $message, $receiptToken);
    }

    /** `DELETE /topics/{topic}/messages/{id}?receiptToken=`. `$message` is what `consume` returned, or its id. */
    public function ack($topic, $message, $receiptToken = null, $cancel = null)
    {
        list($id, $token) = self::reference($message, $receiptToken);
        $this->send(Calls::ack($topic, $id, $token), $cancel);
    }

    /** `PUT /topics/{topic}/messages/{id}/visibility?receiptToken=`. */
    public function changeVisibility($topic, $message, $seconds, $receiptToken = null, $cancel = null)
    {
        list($id, $token) = self::reference($message, $receiptToken);
        $reply = $this->send(Calls::changeVisibility($topic, $id, $token, $seconds), $cancel);
        return Parser::visibility($reply->body);
    }

    /**
     * `POST /topics/{topic}/messages/ack`: acknowledges many messages `consume` returned, in one request.
     *
     * @param Message[] $messages
     * @return BatchResult[]
     */
    public function ackBatch($topic, array $messages, $cancel = null)
    {
        return Parser::batchResults($this->send(Calls::receipts($topic, 'ack', $messages), $cancel)->body);
    }

    /**
     * `POST /topics/{topic}/messages/visibility`: the invisibility of each message, `$seconds` from now.
     *
     * @param Message[] $messages
     * @return BatchResult[]
     */
    public function changeVisibilityBatch($topic, array $messages, $seconds, $cancel = null)
    {
        $call = Calls::receipts($topic, 'visibility', $messages, $seconds);
        return Parser::batchResults($this->send($call, $cancel)->body);
    }

    /**
     * Reads from the queue and hands each message to `$handler($context)`. The message is deleted if the handler
     * returns without throwing (`$context->delete = false` keeps it). Defaults: one message at a time, 20 s of
     * long polling, 120 s of invisibility. PHP runs one thread, so the invisibility is NOT renewed while the
     * handler runs: size the `visibilityTimeout` for it, or call `$context->changeVisibility()`. If the handler
     * throws, the message is not deleted and the exception propagates. Returns how many messages were handled.
     *
     * @param callable                  $handler `function (MessageContext $context)`
     * @param ConsumeOptions|array|null $options
     */
    public function consumeWith($topic, $handler, $options = null, $cancel = null)
    {
        return $this->handleAll($topic, $handler, $options, $cancel, false);
    }

    /**
     * `consumeWith` with the body already parsed (`json_decode`, objects as arrays) in `$context->body`. A body
     * that does not parse gives `client.invalid-response` before the handler is called, and the message is not
     * deleted.
     */
    public function consumeJson($topic, $handler, $options = null, $cancel = null)
    {
        return $this->handleAll($topic, $handler, $options, $cancel, true);
    }

    private function handleAll($topic, $handler, $options, $cancel, $parse)
    {
        $opts = Calls::handlerDefaults(ConsumeOptions::from($options));
        $handled = 0;
        foreach ($this->fetch($topic, $opts, $cancel) as $message) {
            $body = null;
            if ($parse) {
                $body = json_decode($message->body, true);
                if ($body === null && json_last_error() !== JSON_ERROR_NONE) {
                    throw TabaException::client(
                        'client.invalid-response',
                        'body of message ' . $message->id . ' is not valid JSON'
                    );
                }
            }
            $client = $this;
            $context = new MessageContext($message, $opts->visibilityTimeout, function ($seconds) use ($client, $topic, $message, $cancel) {
                $client->changeVisibility($topic, $message, $seconds, null, $cancel);
            });
            $context->body = $body;
            call_user_func($handler, $context);
            if ($context->delete) {
                $this->settle($topic, $message);
            }
            $handled++;
        }
        return $handled;
    }

    /** Without the token: the message was already handled, and acknowledging it must not be cut short. */
    private function settle($topic, Message $message)
    {
        try {
            $this->ack($topic, $message);
        } catch (TabaException $e) {
            if ($e->status !== 409 && $e->status !== 404) {
                throw $e;
            }
        }
    }

    /**
     * The consume loop: long polling (`waitTime` 20 and one message at a time, if not set), calls
     * `$handler($message)` and acknowledges by itself only if it returns without throwing (a message that fails
     * goes back to the queue at the end of the `visibilityTimeout`, and the exception goes to
     * `$onError($message, $exception)`). Blocks until `$cancel` is cancelled (from the handler, or by
     * `cancelAfter`) and then returns without an exception. A server error while consuming propagates.
     *
     * @param callable                  $handler `function (Message $message)`
     * @param ConsumeOptions|array|null $options
     * @param callable|null             $onError `function (Message $message, \Exception $error)`
     */
    public function subscribe($topic, $handler, $options = null, $cancel = null, $onError = null)
    {
        $cancel = $cancel !== null ? $cancel : new CancelToken();
        $poll = Calls::subscribeDefaults(ConsumeOptions::from($options));
        while (!$cancel->isCancelled()) {
            try {
                $messages = $this->fetch($topic, $poll, $cancel);
            } catch (Cancelled $e) {
                return;
            }
            foreach ($messages as $message) {
                $handled = false;
                try {
                    call_user_func($handler, $message);
                    $handled = true;
                } catch (\Exception $e) { // the handler is the application's code
                    if ($onError !== null) {
                        call_user_func($onError, $message, $e);
                    }
                }
                if ($handled) {
                    $this->settle($topic, $message);
                }
                if ($cancel->isCancelled()) {
                    return;
                }
            }
        }
    }
}
