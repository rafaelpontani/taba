<?php
/** The models of the client API and the reading of its JSON responses. */

namespace Taba;

class Topic
{
    public $id;
    public $name;
    /** `queue` or `distributor`. */
    public $type;
    /** Creation date, as RFC 3339 text. */
    public $createdAt;
    public $fifo;
    public $volatile;
    /** The topic JSON, as the server returned it. */
    public $raw;
}

/** What `createOrUpdateTopic` did and the topic as it is now. */
class TopicPut
{
    /** It did not exist (`201`). */
    const CREATED = 'created';
    /** It existed: the mutable fields are now the declared ones, or already were (`200`). */
    const UPDATED = 'updated';
    /** It was removed and created again with a new `id` (`201` with `Taba-Topic-Recreated: true`). */
    const RECREATED = 'recreated';

    /** @var Topic */
    public $topic;
    /** `created`, `updated` or `recreated`. */
    public $outcome;

    public function __construct($topic, $outcome)
    {
        $this->topic = $topic;
        $this->outcome = $outcome;
    }
}

class Page
{
    /** @var Topic[] */
    public $items = array();
    /** The cursor of the next page, or null on the last one. */
    public $nextCursor;
    public $raw;
}

/** A delivered message. The `body` always comes out as bytes (doc 21, "The delivered message"). */
class Message
{
    public $id;
    /** JSON: the JSON text as it came. `text/*`: the UTF-8 text. base64: the bytes already decoded. */
    public $body = '';
    public $contentType;
    public $receiptToken;
    public $deliveryCount = 0;
    /** As RFC 3339 text. */
    public $publishedAt = '';
    public $invisibleUntil = '';
    public $groupId;
    public $deduplicationId;
    public $deadLetteredAt;
    public $deadLetterReason;
    /** The message JSON, as the server returned it. */
    public $raw;

    /** The `body` as UTF-8 text. */
    public function bodyText()
    {
        return $this->body;
    }

    /** The `body` read as JSON (`json_decode`; objects as arrays unless `$assoc` is false). */
    public function json($assoc = true)
    {
        $value = json_decode($this->body, $assoc);
        if ($value === null && json_last_error() !== JSON_ERROR_NONE) {
            throw TabaException::client('client.invalid-response', 'body of message ' . $this->id . ' is not valid JSON');
        }
        return $value;
    }
}

/** The copy of a publication made by a distributor to a queue. */
class DistributorDelivery
{
    public $topic;
    public $id;
    public $errorCode;
    public $errorMessage;

    public function succeeded()
    {
        return $this->errorCode === null;
    }
}

class PublishResponse
{
    /** The message `id` (queue). Null in a distributor's response. */
    public $id;
    /** FIFO queue only: true when it repeated a publication within the deduplication window. */
    public $duplicate;
    /** Distributor only: how many queues matched. */
    public $matched;
    /** @var DistributorDelivery[] */
    public $deliveries = array();
    public $raw;
}

class VisibilityResponse
{
    public $id;
    public $invisibleUntil;
    public $raw;
}

/**
 * The result of one item of a batch, in the order of the request: the `status` the single call would have
 * answered and what it would have returned. An item that failed does not throw: `ok()` is false.
 */
class BatchResult
{
    /** 201 published, 200 a FIFO duplicate, a distributor or a new invisibility, 204 acked, or the error's. */
    public $status = 0;
    public $id;
    public $duplicate;
    public $matched;
    public $invisibleUntil;
    public $errorCode;
    public $errorMessage;
    public $raw;

    public function ok()
    {
        return $this->errorCode === null && $this->status < 300;
    }
}

/**
 * What the `consumeWith` handler receives: the `message`, the `delete` flag (the message is deleted at the end
 * if the handler returns without throwing; the default is true) and `changeVisibility()`.
 *
 * PHP runs one thread, so the connector cannot renew the invisibility while the handler runs: give the message
 * enough time with the `visibilityTimeout` option (120 s by default) or call `changeVisibility()` from the handler.
 */
class MessageContext
{
    public $message;
    public $delete = true;
    /** `consumeJson` only: the parsed body. */
    public $body;
    private $leaseSeconds;
    private $change;

    public function __construct(Message $message, $leaseSeconds, $change)
    {
        $this->message = $message;
        $this->leaseSeconds = $leaseSeconds;
        $this->change = $change;
    }

    /** The seconds of invisibility the message was taken with (or last changed to). */
    public function leaseSeconds()
    {
        return $this->leaseSeconds;
    }

    /** Changes the invisibility to `$seconds` from now on. */
    public function changeVisibility($seconds)
    {
        $call = $this->change;
        $call($seconds);
        $this->leaseSeconds = $seconds;
    }
}

/** Reading the responses. */
final class Parser
{
    private static function invalid($what, JsonError $e)
    {
        return TabaException::client('client.invalid-response', $what . ': ' . $e->getMessage());
    }

    private static function fields($raw, $what)
    {
        try {
            return JsonFields::parse($raw);
        } catch (JsonError $e) {
            throw self::invalid($what, $e);
        }
    }

    private static function isJsonType($contentType)
    {
        $parts = explode(';', $contentType);
        $media = strtolower(trim($parts[0]));
        return $media === 'application/json' || substr($media, -5) === '+json';
    }

    private static function orEmpty($value)
    {
        return $value === null ? '' : $value;
    }

    public static function topic($raw)
    {
        $f = self::fields($raw, 'topic');
        try {
            $t = new Topic();
            $t->id = self::orEmpty($f->string('id'));
            $t->name = self::orEmpty($f->string('name'));
            $t->type = self::orEmpty($f->string('type'));
            $t->createdAt = self::orEmpty($f->string('createdAt'));
            $t->fifo = $f->boolean('fifo');
            $t->volatile = $f->boolean('volatile');
            $t->raw = $raw;
            return $t;
        } catch (JsonError $e) {
            throw self::invalid('topic', $e);
        }
    }

    public static function message($raw)
    {
        $f = self::fields($raw, 'message');
        try {
            $contentType = $f->string('contentType');
            $contentType = $contentType === null ? 'application/json' : $contentType;
            $body = $f->raw('body');
            if ($body === null) {
                $data = '';
            } elseif ($f->string('bodyEncoding') === 'base64') {
                $data = base64_decode(Json::decodeString($body), true);
                if ($data === false) {
                    throw TabaException::client('client.invalid-response', 'invalid base64 body');
                }
            } elseif (self::isJsonType($contentType)) {
                $data = $body;
            } else {
                $data = Json::decodeString($body);
            }
            $m = new Message();
            $m->id = self::orEmpty($f->string('id'));
            $m->body = $data;
            $m->contentType = $contentType;
            $m->receiptToken = self::orEmpty($f->string('receiptToken'));
            $count = $f->integer('deliveryCount');
            $m->deliveryCount = $count === null ? 0 : $count;
            $m->publishedAt = self::orEmpty($f->string('publishedAt'));
            $m->invisibleUntil = self::orEmpty($f->string('invisibleUntil'));
            $m->groupId = $f->string('groupId');
            $m->deduplicationId = $f->string('deduplicationId');
            $m->deadLetteredAt = $f->string('deadLetteredAt');
            $m->deadLetterReason = $f->string('deadLetterReason');
            $m->raw = $raw;
            return $m;
        } catch (JsonError $e) {
            throw self::invalid('message', $e);
        }
    }

    public static function messages($raw)
    {
        $f = self::fields($raw, 'consume');
        try {
            $listing = $f->raw('messages');
            $out = array();
            foreach ($listing === null ? array() : Json::items($listing) as $item) {
                $out[] = self::message($item);
            }
            return $out;
        } catch (JsonError $e) {
            throw self::invalid('messages', $e);
        }
    }

    public static function page($raw)
    {
        $f = self::fields($raw, 'page');
        try {
            $page = new Page();
            $listing = $f->raw('items');
            foreach ($listing === null ? array() : Json::items($listing) as $item) {
                $page->items[] = self::topic($item);
            }
            $page->nextCursor = $f->string('nextCursor');
            $page->raw = $raw;
            return $page;
        } catch (JsonError $e) {
            throw self::invalid('page items', $e);
        }
    }

    /** The `error` object of a delivery or a batch item: array(code, message), or null when there is none. */
    private static function errorOf(JsonFields $f)
    {
        $error = $f->raw('error');
        if ($error === null || $error[0] !== '{') {
            return null;
        }
        $e = JsonFields::parse($error);
        $code = $e->string('code');
        return array($code === null ? '' : $code, $e->string('message'));
    }

    public static function publishResponse($raw)
    {
        $f = self::fields($raw, 'publication');
        try {
            $r = new PublishResponse();
            $r->id = $f->string('id');
            $r->duplicate = $f->boolean('duplicate');
            $r->matched = $f->integer('matched');
            $r->raw = $raw;
            $listing = $f->raw('deliveries');
            foreach ($listing === null ? array() : Json::items($listing) as $item) {
                $d = JsonFields::parse($item);
                $delivery = new DistributorDelivery();
                $delivery->topic = self::orEmpty($d->string('topic'));
                $delivery->id = $d->string('id');
                $error = self::errorOf($d);
                if ($error !== null) {
                    $delivery->errorCode = $error[0];
                    $delivery->errorMessage = $error[1];
                }
                $r->deliveries[] = $delivery;
            }
            return $r;
        } catch (JsonError $e) {
            throw self::invalid('publication', $e);
        }
    }

    /** The `results` of a batch, one per item in the order of the request. */
    public static function batchResults($raw)
    {
        $f = self::fields($raw, 'batch');
        try {
            $results = array();
            $listing = $f->raw('results');
            foreach ($listing === null ? array() : Json::items($listing) as $item) {
                $r = JsonFields::parse($item);
                $result = new BatchResult();
                $status = $r->integer('status');
                $result->status = $status === null ? 0 : $status;
                $result->id = $r->string('id');
                $result->duplicate = $r->boolean('duplicate');
                $result->matched = $r->integer('matched');
                $result->invisibleUntil = $r->string('invisibleUntil');
                $error = self::errorOf($r);
                if ($error !== null) {
                    $result->errorCode = $error[0];
                    $result->errorMessage = $error[1];
                }
                $result->raw = $item;
                $results[] = $result;
            }
            return $results;
        } catch (JsonError $e) {
            throw self::invalid('batch results', $e);
        }
    }

    public static function visibility($raw)
    {
        $f = self::fields($raw, 'visibility');
        try {
            $r = new VisibilityResponse();
            $r->id = self::orEmpty($f->string('id'));
            $r->invisibleUntil = self::orEmpty($f->string('invisibleUntil'));
            $r->raw = $raw;
            return $r;
        } catch (JsonError $e) {
            throw self::invalid('visibility', $e);
        }
    }
}
