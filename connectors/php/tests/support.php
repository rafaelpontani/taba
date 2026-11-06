<?php
/** The fake `curl` (the equivalent of the C# `ScriptedHandler`): records every request and answers from a script. */

namespace Taba\Tests;

use Taba\Client;
use Taba\Config;
use Taba\NetworkFailure;

class Recorded
{
    public $method;
    public $path;
    public $rawQuery;
    public $query = array();
    public $headers = array();
    public $body;
    public $timeout;

    public function header($name)
    {
        $name = strtolower($name);
        return isset($this->headers[$name]) ? $this->headers[$name] : null;
    }
}

function reply($status = 200, $body = '', $headers = array())
{
    if (!is_string($body)) {
        $body = json_encode($body);
    }
    return array('status' => $status, 'body' => $body, 'headers' => $headers);
}

class FakeExecutor
{
    /** @var Recorded[] */
    public $requests = array();
    private $script = array();
    private $fallback;

    public function __construct()
    {
        $this->fallback = reply(200, '{}');
    }

    /** Replies are arrays from `reply()`, callables of the Recorded request returning one, or a NetworkFailure. */
    public function reply()
    {
        foreach (func_get_args() as $answer) {
            $this->script[] = $answer;
        }
        return $this;
    }

    public function fallback($answer)
    {
        $this->fallback = $answer;
        return $this;
    }

    /** Answers the consume calls (GET) with these message batches, in order, then with an empty one. */
    public function batches()
    {
        $queue = func_get_args();
        $this->fallback = function (Recorded $r) use (&$queue) {
            if ($r->method !== 'GET') {
                return reply(200, array('id' => 'x', 'invisibleUntil' => 'x'));
            }
            $batch = $queue ? array_shift($queue) : array();
            return reply(200, array('messages' => $batch));
        };
        return $this;
    }

    public function __invoke($url, $method, array $headerLines, $body, $timeout, Config $config, $cancel)
    {
        $r = new Recorded();
        $r->method = $method;
        $parts = parse_url($url);
        $r->path = $parts['path'];
        $r->rawQuery = isset($parts['query']) ? $parts['query'] : '';
        if ($r->rawQuery !== '') {
            foreach (explode('&', $r->rawQuery) as $pair) {
                $kv = explode('=', $pair, 2);
                $r->query[rawurldecode($kv[0])] = rawurldecode($kv[1]);
            }
        }
        foreach ($headerLines as $line) {
            $kv = explode(':', $line, 2);
            $r->headers[strtolower(trim($kv[0]))] = trim($kv[1]);
        }
        $r->body = $body;
        $r->timeout = $timeout;
        $this->requests[] = $r;
        $answer = $this->script ? array_shift($this->script) : $this->fallback;
        if (is_callable($answer)) {
            $answer = call_user_func($answer, $r);
        }
        if ($answer instanceof NetworkFailure) {
            throw $answer;
        }
        $headers = array();
        foreach ($answer['headers'] as $k => $v) {
            $headers[strtolower($k)] = $v;
        }
        return array($answer['status'], $headers, $answer['body']);
    }

    public function last()
    {
        return $this->requests[count($this->requests) - 1];
    }
}

/** A `sleep` that does not wait and records what was asked. */
class FakeSleep
{
    public $waits = array();

    public function __invoke($seconds, $cancel)
    {
        $this->waits[] = $seconds;
        $cancel->throwIfCancelled();
    }
}

abstract class ClientTestCase extends TestCase
{
    /** @var FakeExecutor */
    protected $http;
    /** @var FakeSleep */
    protected $sleep;
    /** @var Client */
    protected $client;

    protected function config()
    {
        return new Config('http://taba.test:8080');
    }

    public function setUp()
    {
        $this->http = new FakeExecutor();
        $this->sleep = new FakeSleep();
        $this->client = new Client($this->config(), $this->sleep, $this->http);
    }
}

function topicJson($name, $extra = array())
{
    return array_merge(array('id' => 't-' . $name, 'name' => $name, 'type' => 'queue', 'createdAt' => '2026-10-27T10:00:00Z'), $extra);
}

/** A message as the server sends it; a string `$body` is JSON text already written. */
function messageJson($id, $body, $extra = array())
{
    return array_merge(array(
        'id' => $id,
        'body' => $body,
        'contentType' => 'application/json',
        'receiptToken' => 'rt-' . $id,
        'deliveryCount' => 1,
        'publishedAt' => '2026-10-27T10:00:00Z',
        'invisibleUntil' => '2026-10-27T10:02:00Z',
    ), $extra);
}

/** A free TCP port nobody listens on. */
function closedPort()
{
    $server = stream_socket_server('tcp://127.0.0.1:0', $errno, $errstr);
    $name = stream_socket_get_name($server, false);
    fclose($server);
    $parts = explode(':', $name);
    return (int) end($parts);
}
