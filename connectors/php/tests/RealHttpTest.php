<?php

namespace Taba\Tests;

use Taba\CancelToken;
use Taba\Cancelled;
use Taba\Client;
use Taba\Config;
use Taba\TopicRequest;
use Taba\TopicUpdate;

/** The `curl` transport itself, against the built-in PHP server (which echoes the request) and raw sockets. */
class RealHttpTest extends TestCase
{
    private static $process;
    private static $port;
    private $sleep;
    private $sockets = array();

    private static function startServer()
    {
        if (self::$process !== null) {
            return;
        }
        self::$port = closedPort();
        $command = escapeshellarg(PHP_BINARY) . ' -S 127.0.0.1:' . self::$port . ' ' . escapeshellarg(__DIR__ . '/echo_router.php');
        $spec = array(0 => array('pipe', 'r'), 1 => array('file', sys_get_temp_dir() . '/taba-php-server.log', 'a'), 2 => array('file', sys_get_temp_dir() . '/taba-php-server.log', 'a'));
        self::$process = proc_open($command, $spec, $pipes);
        register_shutdown_function(function () {
            if (is_resource(RealHttpTest::$process)) {
                proc_terminate(RealHttpTest::$process);
            }
        });
        $deadline = microtime(true) + 10;
        while (microtime(true) < $deadline) {
            $socket = @fsockopen('127.0.0.1', self::$port, $errno, $errstr, 0.2);
            if ($socket) {
                fclose($socket);
                return;
            }
            usleep(50000);
        }
        throw new \RuntimeException('the test server did not start');
    }

    public function setUp()
    {
        self::startServer();
        $this->sleep = new FakeSleep();
    }

    public function tearDown()
    {
        foreach ($this->sockets as $s) {
            fclose($s);
        }
    }

    private function client($token = 'tb_x.secret', $configure = null)
    {
        $config = new Config('http://127.0.0.1:' . self::$port, $token);
        if ($configure) {
            call_user_func($configure, $config);
        }
        return new Client($config, $this->sleep);
    }

    /** Reads the echo of a request: a Topic whose `raw` is the echoed JSON. */
    private function echoOf($topic)
    {
        return json_decode($topic->raw, true);
    }

    /// UC: UC-CNT-001, UC-CNT-003
    public function testTheRequestGoesOnTheWireWithTheRightVerbHeadersAndBody()
    {
        $client = $this->client();
        $e = $this->echoOf($client->createTopic(new TopicRequest('q', 'queue')));
        $this->assertSame('POST', $e['method']);
        $this->assertSame('/topics', $e['uri']);
        $this->assertSame('{"name":"q","type":"queue"}', base64_decode($e['body']));
        $this->assertSame('Bearer tb_x.secret', $e['headers']['authorization']);
        $this->assertSame('application/json', $e['headers']['content-type']);
        $this->assertSame('application/json', $e['headers']['accept']);
        $this->assertContains('taba-php/', $e['headers']['user-agent']);
        $this->assertFalse(isset($e['headers']['expect']), 'no 100-continue');

        $e = $this->echoOf($client->updateTopic('q', new TopicUpdate(array('idleTimeout' => 5))));
        $this->assertSame('PATCH', $e['method'], 'a real PATCH');
        $this->assertFalse(isset($e['headers']['x-http-method-override']));

        $e = $this->echoOf($client->getTopic('a b'));
        $this->assertSame('GET', $e['method']);
        $this->assertSame('/topics/a%20b', $e['uri']);

        $e = $this->echoOf($this->client(null)->getTopic('q'));
        $this->assertFalse(isset($e['headers']['authorization']), 'no token, no header');
        $client->close();
    }

    /// UC: UC-CNT-004
    public function testBinaryBodiesGoOutByteForByte()
    {
        $client = $this->client();
        $payload = '';
        for ($i = 0; $i < 256; $i++) {
            $payload .= chr($i);
        }
        $r = $client->publish('q', $payload, array('contentType' => 'application/octet-stream', 'groupId' => 'g'));
        $echoed = json_decode($r->raw, true);
        $this->assertSame($payload, base64_decode($echoed['body']));
        $this->assertSame('application/octet-stream', $echoed['headers']['content-type']);
        $this->assertSame('/topics/q/messages?groupId=g', $echoed['uri']);
        $big = str_repeat('x', 100000);
        $r = $client->publish('q', $big);
        $this->assertSame($big, base64_decode(json_decode($r->raw, true)['body']));
        $client->close();
    }

    /// UC: UC-CNT-017, UC-CNT-010
    public function testAnErrorResponseBecomesATabaExceptionWithItsHeaders()
    {
        $client = $this->client();
        $e = $this->assertError('echo.status', function () use ($client) {
            $client->getTopic('status-404');
        });
        $this->assertSame(404, $e->status);
        $this->assertSame('echo-1', $e->requestId);
        $this->assertSame(array(), $this->sleep->waits);

        $this->assertError('echo.status', function () use ($client) {
            $client->getTopic('status-503');
        });
        $this->assertSame(array(3.0, 3.0), $this->sleep->waits, 'the Retry-After header is honored');
        $client->close();
    }

    /// UC: UC-CNT-013
    public function testARefusedConnectionIsARetriedNetworkErrorOfTheRealTransport()
    {
        $client = new Client(new Config('http://127.0.0.1:' . closedPort()), $this->sleep);
        $e = $this->assertError('client.network', function () use ($client) {
            $client->getTopic('q');
        });
        $this->assertSame(0, $e->status);
        $this->assertSame(array(1.0, 2.0), $this->sleep->waits);
    }

    private function silentServer()
    {
        $server = stream_socket_server('tcp://127.0.0.1:0', $errno, $errstr);
        $this->sockets[] = $server;
        $parts = explode(':', stream_socket_get_name($server, false));
        return 'http://127.0.0.1:' . end($parts);
    }

    /// UC: UC-CNT-009
    public function testATimeoutOfTheRealTransportIsNotRepeated()
    {
        $client = new Client(Config::from(array('baseUrl' => $this->silentServer(), 'requestTimeout' => 0.4)), $this->sleep);
        $started = microtime(true);
        $this->assertError('client.timeout', function () use ($client) {
            $client->getTopic('q');
        });
        $this->assertLessThan(5, microtime(true) - $started, 'time');
        $this->assertSame(array(), $this->sleep->waits, 'a timeout is not repeated');
    }

    /// UC: UC-CNT-020
    public function testCancellingATokenEndsALongPollingWithoutAClientError()
    {
        $client = new Client(new Config($this->silentServer()), $this->sleep);
        $token = new CancelToken();
        $token->cancelAfter(0.3);
        $started = microtime(true);
        $this->assertThrows('Taba\Cancelled', function () use ($client, $token) {
            $client->consume('q', array('waitTime' => 20), $token);
        });
        $this->assertLessThan(8, microtime(true) - $started, 'the long polling did not wait for its timeout');
        // Already cancelled: no request goes out.
        $this->assertThrows('Taba\Cancelled', function () use ($client, $token) {
            $client->getTopic('q', $token);
        });
    }

    /// UC: UC-CNT-002
    public function testAnInvalidAddressNeverReachesTheNetwork()
    {
        $this->assertError('client.invalid-config', function () {
            new Client(new Config('file:///etc/passwd'));
        });
    }
}
