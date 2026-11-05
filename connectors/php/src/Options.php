<?php
/**
 * The request objects. Each takes an array of its fields in the constructor (an unknown field is a
 * `client.invalid-request` error, so a typo does not go unnoticed) and the methods that take one also accept the
 * array itself:  `$client->consume('orders', array('waitTime' => 20))`.
 */

namespace Taba;

abstract class Fields
{
    protected static function fill($object, $fields)
    {
        if ($fields === null) {
            return;
        }
        if (!is_array($fields)) {
            throw TabaException::client('client.invalid-request', get_class($object) . ' expects an array of fields');
        }
        foreach ($fields as $key => $value) {
            if (!property_exists($object, $key)) {
                throw TabaException::client('client.invalid-request', 'unknown field of ' . get_class($object) . ': ' . $key);
            }
            $object->$key = $value;
        }
    }
}

/** `consume` options. The connector sends only what was set: whatever is null takes the server default. */
class ConsumeOptions extends Fields
{
    public $maxMessages;
    /** Seconds of invisibility for the delivered messages. */
    public $visibilityTimeout;
    /** Seconds of long polling. */
    public $waitTime;
    public $autoCreate = false;
    /** The next four only apply with `autoCreate`. */
    public $volatile;
    public $idleTimeout;
    public $fifo;
    public $tags;
    public $requestId;
    public $traceparent;

    public function __construct($fields = array())
    {
        self::fill($this, $fields);
    }

    /** From a ConsumeOptions, an array of fields or null (a copy: the caller's object is never changed). */
    public static function from($value)
    {
        if ($value instanceof ConsumeOptions) {
            return clone $value;
        }
        return new ConsumeOptions($value);
    }
}

/** The options of `publish`. */
class PublishOptions extends Fields
{
    /** Default `application/json` (`publishText` and `publishJson` set their own). */
    public $contentType;
    public $groupId;
    public $deduplicationId;
    public $requestId;
    public $traceparent;
    /** True when the topic is a FIFO queue: only there can a 502/504 be repeated. */
    public $fifo = false;

    public function __construct($fields = array())
    {
        self::fill($this, $fields);
    }

    public static function from($value)
    {
        if ($value instanceof PublishOptions) {
            return clone $value;
        }
        return new PublishOptions($value);
    }
}

/** `POST /topics`. Only what was filled in is sent. */
class TopicRequest extends Fields
{
    public $name;
    /** `queue` or `distributor`. */
    public $type;
    public $tags;
    /** Distributor only: the name prefixes of the queues each publication is copied to. */
    public $prefixes;
    public $volatile;
    public $idleTimeout;
    public $fifo;
    public $maxMessageBytes;
    public $ttl;
    public $maxDeliveries;

    public function __construct($name = null, $type = null, $fields = array())
    {
        $this->name = $name;
        $this->type = $type;
        self::fill($this, $fields);
    }

    public static function from($value)
    {
        if ($value instanceof TopicRequest) {
            return $value;
        }
        if (!is_array($value)) {
            throw TabaException::client('client.invalid-request', 'request is required');
        }
        $request = new TopicRequest();
        self::fill($request, $value);
        return $request;
    }
}

/**
 * `PATCH /topics/{topic}`: only what was given is sent. For `maxMessageBytes`, `ttl` and `maxDeliveries` a key
 * present with `null` sends `null` (back to the server default); a key left out is not sent.
 *
 *     new TopicUpdate(array('ttl' => null, 'maxDeliveries' => 5));
 */
class TopicUpdate
{
    private static $known = array('tags', 'prefixes', 'idleTimeout', 'maxMessageBytes', 'ttl', 'maxDeliveries');
    private $given = array();

    public function __construct($fields = array())
    {
        foreach ((array) $fields as $key => $value) {
            if (!in_array($key, self::$known, true)) {
                throw TabaException::client('client.invalid-request', 'unknown field of TopicUpdate: ' . $key);
            }
            $this->given[$key] = $value;
        }
    }

    public static function from($value)
    {
        if ($value instanceof TopicUpdate) {
            return $value;
        }
        if (!is_array($value)) {
            throw TabaException::client('client.invalid-request', 'update is required');
        }
        return new TopicUpdate($value);
    }

    /** The fields to send, name => value (null means a JSON null, only for the three nullable ones). */
    public function given()
    {
        return $this->given;
    }
}

/** One message of `publishBatch`: the same fields as `publish`. The bytes travel in base64. */
class BatchMessage extends Fields
{
    public $body;
    /** Default `application/json`. */
    public $contentType;
    public $groupId;
    public $deduplicationId;

    public function __construct($body = '', $fields = array())
    {
        $this->body = $body;
        self::fill($this, $fields);
    }
}
