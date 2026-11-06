<?php
/** Builds the `ApiCall` of each operation. */

namespace Taba;

final class Calls
{
    /** `consume` waits `waitTime` plus this slack (seconds), not the `requestTimeout`. */
    const LONG_POLL_SLACK = 10.0;

    const HANDLER_MAX_MESSAGES = 1;
    const HANDLER_VISIBILITY_TIMEOUT = 120;
    const HANDLER_WAIT_TIME = 20;

    /** JSON for a request: unicode and slashes as they are; an empty array is `{}` where an object is wanted. */
    public static function encode($value)
    {
        $text = json_encode($value, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        if ($text === false) {
            throw TabaException::client('client.invalid-request', 'cannot serialize the request: ' . self::jsonMessage());
        }
        return $text;
    }

    public static function jsonMessage()
    {
        return function_exists('json_last_error_msg') ? json_last_error_msg() : 'error ' . json_last_error();
    }

    private static function map($value)
    {
        return count($value) === 0 ? new \stdClass() : $value;
    }

    private static function text($body)
    {
        return $body === null ? '' : (string) $body;
    }

    public static function createTopic($request)
    {
        $request = TopicRequest::from($request);
        $body = array('name' => (string) $request->name, 'type' => (string) $request->type);
        if ($request->tags) {
            $body['tags'] = $request->tags;
        }
        if ($request->prefixes) {
            $body['prefixes'] = array_values($request->prefixes);
        }
        foreach (array('volatile', 'idleTimeout', 'fifo', 'maxMessageBytes', 'ttl', 'maxDeliveries') as $name) {
            if ($request->$name !== null) {
                $body[$name] = $request->$name;
            }
        }
        $call = new ApiCall('POST', '/topics');
        $call->withJson(self::encode($body));
        $call->recover[] = array(409, 'topic.already-exists');
        return $call;
    }

    public static function getTopic($name)
    {
        return new ApiCall('GET', '/topics/' . Transport::segment($name));
    }

    public static function listTopics($limit, $cursor, $name)
    {
        $call = new ApiCall('GET', '/topics');
        if ($limit !== null) {
            $call->withQuery('limit', $limit);
        }
        if ($cursor !== null) {
            $call->withQuery('cursor', $cursor);
        }
        if ($name !== null) {
            $call->withQuery('name', $name);
        }
        return $call;
    }

    public static function updateTopic($name, $update)
    {
        $update = TopicUpdate::from($update);
        $body = array();
        foreach ($update->given() as $key => $value) {
            if (in_array($key, array('maxMessageBytes', 'ttl', 'maxDeliveries'), true)) {
                $body[$key] = $value; // null goes as JSON null: back to the server default
            } elseif ($value !== null) {
                $body[$key] = $key === 'tags' ? self::map($value) : ($key === 'prefixes' ? array_values($value) : $value);
            }
        }
        $call = new ApiCall('PATCH', '/topics/' . Transport::segment($name));
        return $call->withJson(count($body) === 0 ? '{}' : self::encode($body));
    }

    public static function deleteTopic($name)
    {
        $call = new ApiCall('DELETE', '/topics/' . Transport::segment($name));
        $call->recover[] = array(404, 'topic.not-found');
        return $call;
    }

    public static function publish($topic, $body, PublishOptions $o)
    {
        $call = new ApiCall('POST', '/topics/' . Transport::segment($topic) . '/messages');
        $call->body = self::text($body);
        $call->contentType = $o->contentType ? $o->contentType : 'application/json';
        $call->requestId = $o->requestId;
        $call->traceparent = $o->traceparent;
        $call->retryGateway = (bool) $o->fifo;
        if ($o->groupId !== null) {
            $call->withQuery('groupId', $o->groupId);
        }
        if ($o->deduplicationId !== null) {
            $call->withQuery('deduplicationId', $o->deduplicationId);
        }
        return $call;
    }

    public static function publishBatch($topic, array $messages)
    {
        $items = array();
        foreach ($messages as $m) {
            if (!($m instanceof BatchMessage)) {
                throw TabaException::client('client.invalid-request', 'publishBatch expects BatchMessage objects');
            }
            $item = array('body' => base64_encode(self::text($m->body)), 'bodyEncoding' => 'base64');
            foreach (array('contentType', 'groupId', 'deduplicationId') as $name) {
                if ($m->$name !== null) {
                    $item[$name] = $m->$name;
                }
            }
            $items[] = $item;
        }
        $call = new ApiCall('POST', '/topics/' . Transport::segment($topic) . '/messages/batch');
        return $call->withJson(self::encode(array('messages' => $items)));
    }

    /** The ack (`$route` `ack`) or the change of invisibility (`visibility`) of many messages in one request. */
    public static function receipts($topic, $route, array $messages, $seconds = null)
    {
        $items = array();
        foreach ($messages as $m) {
            $item = array('id' => $m->id, 'receiptToken' => $m->receiptToken);
            if ($seconds !== null) {
                $item['visibilityTimeout'] = (int) $seconds;
            }
            $items[] = $item;
        }
        $call = new ApiCall('POST', '/topics/' . Transport::segment($topic) . '/messages/' . $route);
        return $call->withJson(self::encode(array('messages' => $items)));
    }

    public static function consume($topic, ConsumeOptions $o)
    {
        $call = new ApiCall('GET', '/topics/' . Transport::segment($topic) . '/messages');
        if ($o->maxMessages !== null) {
            $call->withQuery('maxMessages', $o->maxMessages);
        }
        if ($o->visibilityTimeout !== null) {
            $call->withQuery('visibilityTimeout', $o->visibilityTimeout);
        }
        if ($o->waitTime !== null) {
            $call->withQuery('waitTime', $o->waitTime);
        }
        if ($o->autoCreate) {
            $call->withQuery('autoCreate', true);
            if ($o->volatile !== null) {
                $call->withQuery('volatile', (bool) $o->volatile);
            }
            if ($o->idleTimeout !== null) {
                $call->withQuery('idleTimeout', $o->idleTimeout);
            }
            if ($o->fifo !== null) {
                $call->withQuery('fifo', (bool) $o->fifo);
            }
            if ($o->tags) {
                ksort($o->tags);
                foreach ($o->tags as $key => $value) {
                    $call->withQuery('tag.' . $key, $value);
                }
            }
        }
        $call->requestId = $o->requestId;
        $call->traceparent = $o->traceparent;
        $call->timeout = (float) ($o->waitTime === null ? 0 : $o->waitTime) + self::LONG_POLL_SLACK;
        return $call;
    }

    public static function ack($topic, $id, $receiptToken)
    {
        $call = new ApiCall('DELETE', '/topics/' . Transport::segment($topic) . '/messages/' . Transport::segment($id));
        return $call->withQuery('receiptToken', $receiptToken);
    }

    public static function changeVisibility($topic, $id, $receiptToken, $seconds)
    {
        $call = new ApiCall('PUT', '/topics/' . Transport::segment($topic) . '/messages/' . Transport::segment($id) . '/visibility');
        $call->withQuery('receiptToken', $receiptToken);
        return $call->withJson(self::encode(array('visibilityTimeout' => (int) $seconds)));
    }

    /** One message at a time, 20 s of long polling and 120 s of invisibility. */
    public static function handlerDefaults(ConsumeOptions $o)
    {
        $o->maxMessages = $o->maxMessages === null ? self::HANDLER_MAX_MESSAGES : $o->maxMessages;
        $o->visibilityTimeout = $o->visibilityTimeout === null ? self::HANDLER_VISIBILITY_TIMEOUT : $o->visibilityTimeout;
        $o->waitTime = $o->waitTime === null ? self::HANDLER_WAIT_TIME : $o->waitTime;
        return $o;
    }

    public static function subscribeDefaults(ConsumeOptions $o)
    {
        $o->waitTime = $o->waitTime === null ? self::HANDLER_WAIT_TIME : $o->waitTime;
        $o->maxMessages = $o->maxMessages === null ? self::HANDLER_MAX_MESSAGES : $o->maxMessages;
        return $o;
    }
}
