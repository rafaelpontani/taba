<?php
/**
 * The PHP driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and
 * runs each line against a real Taba through the connector, in the style of the language.
 *
 *     php contract/driver.php --url http://host:8080 --token <token> --prefix ct-php-0101 --lang php \
 *         --scenario ../contract/scenarios/core.scn
 */

require __DIR__ . '/../src/autoload.php';

use Taba\BatchMessage;
use Taba\CancelToken;
use Taba\Cancelled;
use Taba\Client;
use Taba\Config;
use Taba\Message;
use Taba\MessageContext;
use Taba\TabaException;
use Taba\TopicRequest;
use Taba\TopicUpdate;

error_reporting(E_ALL);
set_error_handler(function ($severity, $message, $file, $line) {
    throw new ErrorException($message, 0, $severity, $file, $line);
});

class SkipLine extends Exception
{
}

$ARGS = array('scenario' => array());
for ($i = 1; $i + 1 < count($argv); $i += 2) {
    $key = substr($argv[$i], 2);
    if ($key === 'scenario') {
        $ARGS['scenario'][] = $argv[$i + 1];
    } else {
        $ARGS[$key] = $argv[$i + 1];
    }
}

// ---- the scenario language ------------------------------------------------------------------------------

/** Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched. */
function expand($value, $variables)
{
    global $ARGS;
    $out = '';
    $last = 0;
    preg_match_all('/\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)/', $value, $matches, PREG_SET_ORDER | PREG_OFFSET_CAPTURE);
    foreach ($matches as $m) {
        $start = $m[0][1];
        $out .= rawurldecode(substr($value, $last, $start - $last));
        if ($m[0][0] === '{p}') {
            $out .= $ARGS['prefix'];
        } elseif ($m[0][0] === '{lang}') {
            $out .= $ARGS['lang'];
        } else {
            $out .= isset($variables[$m[1][0]][$m[2][0]]) ? $variables[$m[1][0]][$m[2][0]] : '';
        }
        $last = $start + strlen($m[0][0]);
    }
    return $out . rawurldecode(substr($value, $last));
}

function parseLine($text, $variables)
{
    $parts = explode(' => ', $text, 2);
    $head = preg_split('/\s+/', trim($parts[0]));
    $params = array();
    foreach (array_slice($head, 1) as $pair) {
        $kv = explode('=', $pair, 2);
        $params[$kv[0]] = expand(isset($kv[1]) ? $kv[1] : '', $variables);
    }
    $expects = array();
    if (isset($parts[1])) {
        foreach (preg_split('/\s+/', trim($parts[1])) as $token) {
            if (preg_match('/^([^=~]*)([=~])(.*)$/s', $token, $m)) {
                $expects[] = array($m[1], $m[2], expand($m[3], $variables));
            }
        }
    }
    return array($head[0], $params, $expects);
}

// ---- helpers --------------------------------------------------------------------------------------------

function num($p, $key)
{
    return isset($p[$key]) ? (int) $p[$key] : null;
}

function flag($p, $key)
{
    return isset($p[$key]) ? $p[$key] === 'true' : null;
}

function items($p, $key)
{
    return isset($p[$key]) ? array_values(array_filter(explode(',', $p[$key]), 'strlen')) : null;
}

function tagsOf($p, $key)
{
    if (!isset($p[$key])) {
        return null;
    }
    $tags = array();
    foreach (explode(',', $p[$key]) as $t) {
        $kv = explode(':', $t, 2);
        $tags[$kv[0]] = $kv[1];
    }
    return $tags;
}

/** Only what the scenario gave goes into the fields array: absent leaves the field out, "null" is a null. */
function nullableFields($p, $names)
{
    $fields = array();
    foreach ($names as $name) {
        if (isset($p[$name])) {
            $fields[$name] = $p[$name] === 'null' ? null : (int) $p[$name];
        }
    }
    return $fields;
}

function render(Message $m)
{
    $parts = explode(';', $m->contentType);
    if (strtolower(trim($parts[0])) === 'application/octet-stream') {
        return 'b64:' . base64_encode($m->body);
    }
    return $m->bodyText();
}

function text($value)
{
    if ($value === null) {
        return '';
    }
    return is_bool($value) ? ($value ? 'true' : 'false') : (string) $value;
}

function topicRequest($p)
{
    return new TopicRequest($p['name'], $p['type'], array(
        'fifo' => flag($p, 'fifo'), 'prefixes' => items($p, 'prefixes'), 'tags' => tagsOf($p, 'tags'),
        'volatile' => flag($p, 'volatile'), 'ttl' => num($p, 'ttl'), 'maxDeliveries' => num($p, 'maxDeliveries'),
        'maxMessageBytes' => num($p, 'maxMessageBytes'), 'idleTimeout' => num($p, 'idleTimeout'),
    ));
}

function topicFields($t)
{
    return array('id' => $t->id, 'name' => $t->name, 'type' => $t->type, 'fifo' => text($t->fifo), 'volatile' => text($t->volatile), 'raw' => $t->raw);
}

function publishFields($r)
{
    return array('id' => text($r->id), 'duplicate' => text($r->duplicate), 'matched' => text($r->matched));
}

function consumeOptions($p)
{
    return array('maxMessages' => num($p, 'max'), 'waitTime' => num($p, 'wait'), 'visibilityTimeout' => num($p, 'visibility'));
}

function messageFields($messages)
{
    $bodies = array();
    foreach ($messages as $m) {
        $bodies[] = render($m);
    }
    $fields = array('count' => (string) count($messages), 'bodies' => implode(',', $bodies));
    if ($messages) {
        $first = $messages[0];
        $fields['id'] = $first->id;
        $fields['receiptToken'] = $first->receiptToken;
        $fields['deliveryCount'] = (string) $first->deliveryCount;
        $fields['contentType'] = $first->contentType;
        $fields['body'] = render($first);
    }
    return $fields;
}

function publishOptions($p)
{
    return array(
        'groupId' => isset($p['groupId']) ? $p['groupId'] : null,
        'deduplicationId' => isset($p['deduplicationId']) ? $p['deduplicationId'] : null,
        'fifo' => flag($p, 'fifo') ? true : false,
    );
}

function statuses($results)
{
    $codes = array();
    foreach ($results as $r) {
        $codes[] = $r->status;
    }
    return array('count' => (string) count($results), 'statuses' => implode(',', $codes));
}

// ---- the operations -------------------------------------------------------------------------------------

$OPERATIONS = array(
    'createTopic' => function (Client $c, $p) {
        return topicFields($c->createTopic(topicRequest($p)));
    },
    'createOrUpdateTopic' => function (Client $c, $p) {
        $put = $c->createOrUpdateTopic(topicRequest($p), isset($p['recreate']) ? $p['recreate'] : null);
        $fields = topicFields($put->topic);
        $fields['outcome'] = $put->outcome;
        return $fields;
    },
    'getTopic' => function (Client $c, $p) {
        return topicFields($c->getTopic($p['name']));
    },
    'listTopics' => function (Client $c, $p) {
        $topics = $c->listAllTopics(isset($p['name']) ? $p['name'] : null);
        $names = array();
        foreach ($topics as $t) {
            $names[] = $t->name;
        }
        sort($names, SORT_STRING);
        return array('count' => (string) count($topics), 'names' => implode(',', $names));
    },
    'updateTopic' => function (Client $c, $p) {
        $fields = nullableFields($p, array('ttl', 'maxDeliveries', 'maxMessageBytes'));
        if (items($p, 'prefixes') !== null) {
            $fields['prefixes'] = items($p, 'prefixes');
        }
        if (tagsOf($p, 'tags') !== null) {
            $fields['tags'] = tagsOf($p, 'tags');
        }
        if (num($p, 'idleTimeout') !== null) {
            $fields['idleTimeout'] = num($p, 'idleTimeout');
        }
        return topicFields($c->updateTopic($p['name'], new TopicUpdate($fields)));
    },
    'deleteTopic' => function (Client $c, $p) {
        if (flag($p, 'ifEmpty') === true) {
            $c->deleteTopicIfEmpty($p['name']);
        } else {
            $c->deleteTopic($p['name']);
        }
        return array();
    },
    'publishText' => function (Client $c, $p) {
        return publishFields($c->publishText($p['topic'], $p['body'], publishOptions($p)));
    },
    'publishJson' => function (Client $c, $p) {
        return publishFields($c->publishRawJson($p['topic'], $p['body'], publishOptions($p)));
    },
    'publishBytes' => function (Client $c, $p) {
        $options = publishOptions($p);
        $options['contentType'] = isset($p['contentType']) ? $p['contentType'] : 'application/octet-stream';
        return publishFields($c->publish($p['topic'], base64_decode($p['base64']), $options));
    },
    // A batch of texts (one item per text); bad=true adds an item with an invalid content type.
    'publishBatch' => function (Client $c, $p) {
        $batch = array();
        foreach (items($p, 'texts') as $t) {
            $batch[] = new BatchMessage($t, array('contentType' => 'text/plain; charset=utf-8', 'groupId' => isset($p['groupId']) ? $p['groupId'] : null));
        }
        if (flag($p, 'bad')) {
            $batch[] = new BatchMessage('x', array('contentType' => 'not a type'));
        }
        return statuses($c->publishBatch($p['topic'], $batch));
    },
    // Consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes the receipt
    // of the first one old before the batch: it is given back and taken again.
    'settleBatch' => function (Client $c, $p) {
        $messages = $c->consume($p['topic'], consumeOptions($p));
        if (flag($p, 'stale') && $messages) {
            $c->changeVisibility($p['topic'], $messages[0], 0);
            $c->consume($p['topic'], array('maxMessages' => 1, 'waitTime' => 2, 'visibilityTimeout' => num($p, 'visibility')));
        }
        if ($p['action'] === 'visibility') {
            $results = $c->changeVisibilityBatch($p['topic'], $messages, (int) $p['seconds']);
        } else {
            $results = $c->ackBatch($p['topic'], $messages);
        }
        return statuses($results);
    },
    'consume' => function (Client $c, $p) {
        return messageFields($c->consume($p['topic'], consumeOptions($p)));
    },
    'ack' => function (Client $c, $p) {
        $c->ack($p['topic'], $p['id'], $p['receipt']);
        return array();
    },
    'changeVisibility' => function (Client $c, $p) {
        $r = $c->changeVisibility($p['topic'], $p['id'], (int) $p['seconds'], $p['receipt']);
        return array('id' => $r->id, 'invisibleUntil' => $r->invisibleUntil);
    },
    // Consuming with a handler: mode ok (deleted), fail (throws) or keep (delete = false). Mode slow holds the
    // message for a while, which only works if the invisibility is renewed while the handler runs: PHP has one
    // thread and cannot, so that line is skipped.
    'handle' => function (Client $c, $p) {
        if ($p['mode'] === 'slow') {
            // Takes the message out of the queue, so the lines after this one still see what they expect.
            foreach ($c->consume($p['topic'], consumeOptions($p)) as $m) {
                $c->ack($p['topic'], $m);
            }
            throw new SkipLine('PHP cannot renew the invisibility while a handler runs (one thread)');
        }
        $seen = array();
        $failed = false;
        $handled = 0;
        try {
            $handled = $c->consumeWith($p['topic'], function (MessageContext $context) use ($p, &$seen) {
                $seen[] = render($context->message);
                if ($p['mode'] === 'fail') {
                    throw new RuntimeException('the handler failed');
                }
                if ($p['mode'] === 'keep') {
                    $context->delete = false;
                }
            }, consumeOptions($p));
        } catch (TabaException $e) {
            throw $e;
        } catch (Exception $e) { // the handler's own failure
            $failed = true;
        }
        return array('handled' => (string) $handled, 'failed' => text($failed), 'bodies' => implode(',', $seen), 'leaseLost' => 'false');
    },
    // The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first one fail.
    'subscribe' => function (Client $c, $p) {
        $cancel = new CancelToken();
        $cancel->cancelAfter((float) $p['timeout']);
        $seen = array();
        $c->subscribe($p['topic'], function (Message $m) use ($p, $cancel, &$seen) {
            $seen[] = render($m);
            if (count($seen) >= (int) $p['count']) {
                $cancel->cancel();
            }
            if (isset($p['failFirst']) && $p['failFirst'] === 'true' && count($seen) === 1) {
                throw new RuntimeException('the handler failed');
            }
        }, array('visibilityTimeout' => num($p, 'visibility')), $cancel);
        return array('count' => (string) count($seen), 'bodies' => implode(',', $seen));
    },
    // A long polling cancelled from outside must end at once, not at the end of the wait.
    'cancelConsume' => function (Client $c, $p) {
        $cancel = new CancelToken();
        $cancel->cancelAfter((float) $p['after']);
        $started = microtime(true);
        $cancelled = false;
        try {
            $c->consume($p['topic'], array('waitTime' => 20), $cancel);
        } catch (Cancelled $e) {
            $cancelled = true;
        }
        return array('cancelled' => text($cancelled), 'fast' => text(microtime(true) - $started < 10));
    },
    // Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
    'drain' => function (Client $c, $p) {
        $seen = array();
        $deadline = microtime(true) + (float) $p['timeout'];
        while (count($seen) < (int) $p['count'] && microtime(true) < $deadline) {
            foreach ($c->consume($p['topic'], array('maxMessages' => 10, 'waitTime' => 1, 'visibilityTimeout' => 60)) as $m) {
                $seen[] = render($m);
                $c->ack($p['topic'], $m);
            }
        }
        sort($seen, SORT_STRING);
        return array('count' => (string) count($seen), 'bodies' => implode(',', $seen));
    },
    'sleep' => function (Client $c, $p) {
        usleep((int) ((float) $p['seconds'] * 1000000));
        return array();
    },
);

// ---- the run --------------------------------------------------------------------------------------------

function check($result, $expects)
{
    $problems = array();
    $failure = isset($result['error']);
    $expectsError = false;
    foreach ($expects as $e) {
        if ($e[0] === 'error') {
            $expectsError = true;
        }
    }
    if ($failure && !$expectsError) {
        $problems[] = 'unexpected error ' . $result['error'] . ' (status ' . $result['status'] . '): ' . $result['message'];
    }
    if (!$failure && $expectsError) {
        $problems[] = 'expected an error, but the call succeeded';
    }
    foreach ($expects as $e) {
        list($key, $op, $value) = $e;
        $actual = isset($result[$key]) ? $result[$key] : null;
        if ($op === '~') {
            $mismatch = strpos((string) $actual, $value) === false;
        } elseif ($value === '*') {
            $mismatch = $actual === null || $actual === '';
        } else {
            $mismatch = $actual !== $value;
        }
        if ($mismatch) {
            $problems[] = $key . ' ' . $op . ' ' . var_export($value, true) . ', got ' . var_export($actual, true);
        }
    }
    return $problems;
}

$clients = array();
$clientFor = function ($p) use (&$clients, $ARGS) {
    $key = (isset($p['url']) ? $p['url'] : '') . '|' . (isset($p['token']) ? $p['token'] : '') . '|' . (isset($p['attempts']) ? $p['attempts'] : '');
    if (!isset($clients[$key])) {
        $config = new Config(isset($p['url']) ? $p['url'] : $ARGS['url'], isset($p['token']) ? $p['token'] : $ARGS['token']);
        if (isset($p['attempts'])) {
            $config->maxAttempts = (int) $p['attempts'];
        }
        $clients[$key] = new Client($config);
    }
    return $clients[$key];
};

$variables = array();
$passed = $failed = $skipped = 0;
foreach ($ARGS['scenario'] as $path) {
    foreach (file($path, FILE_IGNORE_NEW_LINES) as $n => $raw) {
        $text = trim($raw);
        if ($text === '' || $text[0] === '#') {
            continue;
        }
        $where = basename($path) . ':' . ($n + 1);
        list($op, $params, $expects) = parseLine($text, $variables);
        if (!isset($OPERATIONS[$op])) {
            $skipped++;
            echo "SKIP $where $op (not supported by this connector)\n";
            continue;
        }
        try {
            $result = call_user_func($OPERATIONS[$op], $clientFor($params), $params);
        } catch (SkipLine $e) {
            $skipped++;
            echo "SKIP $where $op ({$e->getMessage()})\n";
            continue;
        } catch (TabaException $e) {
            $result = array('error' => $e->errorCode, 'status' => (string) $e->status, 'message' => (string) $e);
        }
        if (isset($params['as'])) {
            $variables[$params['as']] = $result;
        }
        $problems = check($result, $expects);
        if (!$problems) {
            $passed++;
            echo "PASS $where $op\n";
        } else {
            $failed++;
            echo "FAIL $where $text\n";
            foreach ($problems as $problem) {
                echo "       $problem\n";
            }
        }
    }
}
foreach ($clients as $client) {
    $client->close();
}
echo $ARGS['lang'] . ": $passed passed, $failed failed, $skipped skipped\n";
exit($failed === 0 ? 0 : 1);
