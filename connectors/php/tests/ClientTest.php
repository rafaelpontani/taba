<?php

namespace Taba\Tests;

use Taba\BatchMessage;
use Taba\CancelToken;
use Taba\Client;
use Taba\Config;
use Taba\ConsumeOptions;
use Taba\Message;
use Taba\MessageContext;
use Taba\NetworkFailure;
use Taba\PublishOptions;
use Taba\TabaException;
use Taba\TopicRequest;
use Taba\TopicUpdate;

class ClientTest extends ClientTestCase
{
    private function msg($id, $token)
    {
        $m = new Message();
        $m->id = $id;
        $m->receiptToken = $token;
        return $m;
    }

    /// UC: UC-CNT-001, UC-CNT-029
    public function testTheNineOperationsBuildTheExpectedRoutes()
    {
        $this->http->fallback(reply(200, topicJson('a b')));
        $this->client->createTopic(new TopicRequest('a b', 'queue'));
        $this->client->getTopic('a b');
        $this->client->listTopics(5, 'cur', 'ord');
        $this->client->updateTopic('a b', new TopicUpdate(array('idleTimeout' => 9)));
        $this->client->deleteTopic('a b');
        $this->client->publish('a b', '{}');
        $this->client->consume('a b');
        $this->client->ack('a b', 'm 1', 'r/1');
        $this->client->changeVisibility('a b', 'm 1', 30, 'r/1');

        $expected = array(
            array('POST', '/topics'),
            array('GET', '/topics/a%20b'),
            array('GET', '/topics'),
            array('PATCH', '/topics/a%20b'),
            array('DELETE', '/topics/a%20b'),
            array('POST', '/topics/a%20b/messages'),
            array('GET', '/topics/a%20b/messages'),
            array('DELETE', '/topics/a%20b/messages/m%201'),
            array('PUT', '/topics/a%20b/messages/m%201/visibility'),
        );
        $this->assertSame(count($expected), count($this->http->requests), 'requests');
        foreach ($expected as $i => $e) {
            $this->assertSame($e[0], $this->http->requests[$i]->method, 'method ' . $i);
            $this->assertSame($e[1], $this->http->requests[$i]->path, 'path ' . $i);
        }
        $r = $this->http->requests;
        $this->assertSame('5', $r[2]->query['limit']);
        $this->assertSame('cur', $r[2]->query['cursor']);
        $this->assertSame('ord', $r[2]->query['name']);
        $this->assertSame('r/1', $r[7]->query['receiptToken']);
        $this->assertSame('{"idleTimeout":9}', $r[3]->body);
        $this->assertSame('{"visibilityTimeout":30}', $r[8]->body);
        foreach ($r as $request) {
            $this->assertNull($request->header('X-HTTP-Method-Override'), 'a verb is never overridden');
        }
    }

    /// UC: UC-CNT-001
    public function testCreateTopicSendsOnlyWhatWasFilled()
    {
        $this->http->fallback(reply(201, topicJson('q')));
        $this->client->createTopic(new TopicRequest('q', 'queue'));
        $this->assertSame('{"name":"q","type":"queue"}', $this->http->last()->body);

        $this->client->createTopic(array(
            'name' => 'd', 'type' => 'distributor', 'prefixes' => array('a-', 'b-'), 'tags' => array('k' => 'v'),
            'fifo' => true, 'ttl' => 60, 'maxDeliveries' => 3, 'volatile' => false, 'idleTimeout' => 5, 'maxMessageBytes' => 1024,
        ));
        $body = json_decode($this->http->last()->body, true);
        foreach (array('prefixes', 'tags', 'fifo', 'ttl', 'maxDeliveries', 'volatile', 'idleTimeout', 'maxMessageBytes') as $key) {
            $this->assertTrue(array_key_exists($key, $body), $key);
        }
        $this->assertSame(false, $body['volatile']);

        // PATCH: a key left out is not sent, null is a JSON null (back to the server default).
        $this->client->updateTopic('q', new TopicUpdate(array('ttl' => null, 'maxDeliveries' => 4, 'tags' => array())));
        $this->assertSame('{"ttl":null,"maxDeliveries":4,"tags":{}}', $this->http->last()->body);
        $this->client->updateTopic('q', new TopicUpdate());
        $this->assertSame('{}', $this->http->last()->body);
        $this->assertError('client.invalid-request', function () {
            new TopicRequest('q', 'queue', array('typo' => 1));
        });
    }

    /// UC: UC-CNT-018
    public function testHeadersCarryTheAgentAndTheOptionalIds()
    {
        $this->client->publish('q', 'x', array('requestId' => 'req-1', 'traceparent' => '00-aa-bb-01'));
        $this->client->consume('q', array('requestId' => 'req-2'));
        $r = $this->http->requests;
        $this->assertSame('req-1', $r[0]->header('X-Request-Id'));
        $this->assertSame('00-aa-bb-01', $r[0]->header('traceparent'));
        $this->assertSame('application/json', $r[0]->header('Accept'));
        $this->assertContains('taba-php/', $r[0]->header('User-Agent'));
        $this->assertSame('req-2', $r[1]->header('X-Request-Id'));
        $this->assertNull($r[1]->header('traceparent'));
    }

    /// UC: UC-CNT-004, UC-CNT-025
    public function testPublishSendsRawBytesWithContentTypeAndQuery()
    {
        $this->http->fallback(reply(201, array('id' => 'm1', 'duplicate' => true)));
        $payload = "\x00\x01\x02\xff";
        $r = $this->client->publish('q', $payload, new PublishOptions(array(
            'contentType' => 'application/octet-stream', 'groupId' => 'g 1', 'deduplicationId' => 'd1',
        )));
        $this->assertSame('m1', $r->id);
        $this->assertTrue($r->duplicate);
        $sent = $this->http->last();
        $this->assertSame($payload, $sent->body);
        $this->assertSame('application/octet-stream', $sent->header('Content-Type'));
        $this->assertSame('g 1', $sent->query['groupId']);
        $this->assertSame('d1', $sent->query['deduplicationId']);
        // No content type: application/json. The connector does not validate the body.
        $this->client->publish('q', 'not json');
        $this->assertSame('application/json', $this->http->last()->header('Content-Type'));
        $this->client->publishText('q', "ol\xc3\xa1");
        $this->assertSame('text/plain; charset=utf-8', $this->http->last()->header('Content-Type'));
        $this->assertSame("ol\xc3\xa1", $this->http->last()->body);
    }

    /// UC: UC-CNT-041
    public function testTheBatchesSendOneJsonAndReadOneResultPerItem()
    {
        $this->http->reply(
            reply(200, array('results' => array(
                array('status' => 201, 'id' => 'a'),
                array('status' => 200, 'id' => 'b', 'duplicate' => true),
                array('status' => 400, 'error' => array('code' => 'message.invalid-content-type', 'message' => 'bad')),
            ))),
            reply(200, array('results' => array(
                array('status' => 204),
                array('status' => 409, 'error' => array('code' => 'message.receipt-invalid', 'message' => 'old')),
            ))),
            reply(200, array('results' => array(array('status' => 200, 'id' => 'm1', 'invisibleUntil' => '2026-10-27T10:00:30Z'))))
        );
        $results = $this->client->publishBatch('q', array(
            new BatchMessage('one', array('contentType' => 'text/plain; charset=utf-8', 'groupId' => 'g')),
            new BatchMessage("\x00\xff"),
            new BatchMessage('x', array('contentType' => 'not a type')),
        ));
        $this->assertSame(3, count($results));
        $this->assertTrue($results[0]->ok());
        $this->assertTrue($results[1]->duplicate);
        $this->assertFalse($results[2]->ok(), 'the third fails alone');
        $this->assertSame('message.invalid-content-type', $results[2]->errorCode);

        $sent = $this->http->requests[0];
        $this->assertSame('POST', $sent->method);
        $this->assertSame('/topics/q/messages/batch', $sent->path);
        $items = json_decode($sent->body, true);
        $items = $items['messages'];
        $this->assertSame('b25l', $items[0]['body']);
        $this->assertSame('base64', $items[0]['bodyEncoding']);
        $this->assertSame('g', $items[0]['groupId']);
        $this->assertFalse(array_key_exists('contentType', $items[1]), 'a content type nobody set is not sent');

        $messages = array($this->msg('m1', 'r1'), $this->msg('m2', 'r2'));
        $acked = $this->client->ackBatch('q', $messages);
        $this->assertSame(204, $acked[0]->status);
        $this->assertSame('message.receipt-invalid', $acked[1]->errorCode);
        $this->assertSame('/topics/q/messages/ack', $this->http->requests[1]->path);
        $this->assertSame('{"messages":[{"id":"m1","receiptToken":"r1"},{"id":"m2","receiptToken":"r2"}]}', $this->http->requests[1]->body);

        $visible = $this->client->changeVisibilityBatch('q', array($messages[0]), 30);
        $this->assertSame('2026-10-27T10:00:30Z', $visible[0]->invisibleUntil);
        $this->assertSame('/topics/q/messages/visibility', $this->http->requests[2]->path);
        $this->assertSame('{"messages":[{"id":"m1","receiptToken":"r1","visibilityTimeout":30}]}', $this->http->requests[2]->body);
    }

    /// UC: UC-CNT-004
    public function testADistributorAnswerListsEachCopyAndThePartialFailures()
    {
        $this->http->fallback(reply(200, '{"matched":2,"deliveries":[{"topic":"a-1","id":"x"},{"topic":"a-2","error":{"code":"queue.full","message":"full"}}]}'));
        $r = $this->client->publish('d', '{}');
        $this->assertSame(2, $r->matched);
        $this->assertNull($r->id);
        $this->assertTrue($r->deliveries[0]->succeeded());
        $this->assertFalse($r->deliveries[1]->succeeded());
        $this->assertSame('queue.full', $r->deliveries[1]->errorCode);
    }

    /// UC: UC-CNT-002
    public function testConfigDefaultsAndRequiredBaseUrl()
    {
        foreach (array(null, '', '  ', 'localhost:8080', 'ftp://x', 'http://') as $bad) {
            $this->assertError('client.invalid-config', function () use ($bad) {
                new Client(new Config($bad));
            });
        }
        $config = Config::from('http://localhost:8080');
        $this->assertSame(5.0, $config->connectTimeout);
        $this->assertSame(30.0, $config->requestTimeout);
        $this->assertSame(3, $config->maxAttempts);
        $this->assertSame('taba-php/' . Client::VERSION, $config->userAgent);
        $this->assertSame('t', Config::from(array('baseUrl' => 'http://x', 'token' => 't'))->token);
        $this->assertError('client.invalid-config', function () {
            Config::from(array('baseUri' => 'http://x'));
        });
    }

    /// UC: UC-CNT-003
    public function testATokenGoesInEveryCallAndWithoutItNoCallCarriesAuthorization()
    {
        $with = new Client(new Config('http://taba.test', 'tb_x.secret'), $this->sleep, $this->http);
        $with->getTopic('a');
        $with->consume('a');
        foreach ($this->http->requests as $r) {
            $this->assertSame('Bearer tb_x.secret', $r->header('Authorization'));
        }
        $this->http->requests = array();
        $this->client->getTopic('a');
        $this->assertNull($this->http->last()->header('Authorization'));
    }

    /// UC: UC-CNT-028
    public function testUnknownFieldsInResponsesAreIgnoredAndTheContractVersionIsDeclared()
    {
        $this->http->fallback(reply(200, '{"id":"t1","name":"q","type":"queue","createdAt":"x","somethingNew":{"a":[1,2]},"fifo":false}'));
        $topic = $this->client->getTopic('q');
        $this->assertSame('q', $topic->name);
        $this->assertSame(false, $topic->fifo);
        $this->assertNull($topic->volatile);
        $this->assertSame('1', Client::CONTRACT_VERSION);
    }

    /// UC: UC-CNT-005
    public function testConsumeSendsOnlyWhatWasGiven()
    {
        $this->client->consume('q');
        $this->client->consume('q', array('maxMessages' => 5, 'visibilityTimeout' => 60, 'waitTime' => 20));
        $r = $this->http->requests;
        $this->assertSame('', $r[0]->rawQuery, 'no defaults of its own');
        $this->assertSame('5', $r[1]->query['maxMessages']);
        $this->assertSame('60', $r[1]->query['visibilityTimeout']);
        $this->assertSame('20', $r[1]->query['waitTime']);
    }

    /// UC: UC-CNT-005
    public function testAutoCreateParametersAreLeftOutWithoutAutoCreate()
    {
        $this->client->consume('q', array('volatile' => true, 'fifo' => true, 'tags' => array('a' => 'b')));
        $this->client->consume('q', new ConsumeOptions(array(
            'autoCreate' => true, 'volatile' => true, 'idleTimeout' => 30, 'fifo' => false, 'tags' => array('a' => 'b'),
        )));
        $r = $this->http->requests;
        $this->assertSame('', $r[0]->rawQuery);
        $this->assertSame('true', $r[1]->query['autoCreate']);
        $this->assertSame('true', $r[1]->query['volatile']);
        $this->assertSame('30', $r[1]->query['idleTimeout']);
        $this->assertSame('false', $r[1]->query['fifo']);
        $this->assertSame('b', $r[1]->query['tag.a']);
    }

    /// UC: UC-CNT-008
    public function testTheConsumeReadTimeoutIsWaitTimePlusTenSecondsAndNotTheRequestTimeout()
    {
        $this->client->consume('q', array('waitTime' => 20));
        $this->client->consume('q');
        $this->client->getTopic('q');
        $r = $this->http->requests;
        $this->assertSame(30.0, $r[0]->timeout);
        $this->assertSame(10.0, $r[1]->timeout);
        $this->assertSame(30.0, $r[2]->timeout, 'an ordinary call uses the requestTimeout');
        $short = new Client(new Config('http://taba.test'), $this->sleep, $this->http);
        $short->config()->requestTimeout = 2.0;
        $short->consume('q', array('waitTime' => 20));
        $short->getTopic('q');
        $this->assertSame(30.0, $this->http->requests[3]->timeout);
        $this->assertSame(2.0, $this->http->requests[4]->timeout);
    }

    /// UC: UC-CNT-006
    public function testAckAndVisibilityPutTheReceiptTokenInTheQuery()
    {
        $this->http->reply(reply(204, ''), reply(200, array('id' => 'm1', 'invisibleUntil' => 'later')));
        $m = $this->msg('m1', 'a/b c');
        $this->client->ack('q', $m);
        $v = $this->client->changeVisibility('q', $m, 45);
        $this->assertSame('a/b c', $this->http->requests[0]->query['receiptToken']);
        $this->assertSame('a/b c', $this->http->requests[1]->query['receiptToken']);
        $this->assertSame('later', $v->invisibleUntil);
        $this->assertError('client.invalid-request', function () {
            $this->client->ack('q', 'm1');
        });
    }

    /// UC: UC-CNT-007
    public function testListAllTopicsFollowsNextCursorUntilTheLastPage()
    {
        $this->http->reply(
            reply(200, array('items' => array(topicJson('a'), topicJson('b')), 'nextCursor' => 'c1')),
            reply(200, array('items' => array(topicJson('c')), 'nextCursor' => null))
        );
        $all = $this->client->listAllTopics();
        $this->assertSame(3, count($all));
        $this->assertSame('', $this->http->requests[0]->rawQuery);
        $this->assertSame('c1', $this->http->requests[1]->query['cursor']);
    }

    /// UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
    public function testMessagesComeBackAsBytesForJsonTextAndBase64()
    {
        $this->http->fallback(reply(200, '{"messages":[
          {"id":"m1","body":{"a": 1.0,"b":"é"},"contentType":"application/json","receiptToken":"r1","deliveryCount":2,"groupId":"g","deduplicationId":"d","extra":1},
          {"id":"m2","body":"café","contentType":"text/plain; charset=utf-8","receiptToken":"r2"},
          {"id":"m3","body":"AAEC/w==","bodyEncoding":"base64","contentType":"application/octet-stream","receiptToken":"r3"},
          {"id":"m4","body":[1,2],"contentType":"application/vnd.x+json","receiptToken":"r4","deadLetteredAt":"t","deadLetterReason":"max"}
        ]}'));
        $messages = $this->client->consume('q');
        // JSON is the text of the field as it came, not re-formatted.
        $this->assertSame('{"a": 1.0,"b":"é"}', $messages[0]->bodyText());
        $this->assertSame(2, $messages[0]->deliveryCount);
        $this->assertSame('g', $messages[0]->groupId);
        $this->assertSame("caf\xc3\xa9", $messages[1]->bodyText());
        $this->assertSame("\x00\x01\x02\xff", $messages[2]->body);
        $this->assertSame('[1,2]', $messages[3]->bodyText());
        $this->assertSame('max', $messages[3]->deadLetterReason);
        $decoded = $messages[0]->json();
        $this->assertSame(1.0, $decoded['a']);
    }

    /// UC: UC-CNT-026
    public function testAnInvalidAnswerIsAnInvalidResponseErrorAndNotACrash()
    {
        foreach (array('not json', '[]', '{"messages":[{"id":5}]}', '{"messages":[{"id":"m","body":"!!","bodyEncoding":"base64"}]}', '{"messages":{}}') as $body) {
            $this->http->fallback(reply(200, $body));
            $this->assertError('client.invalid-response', function () {
                $this->client->consume('q');
            });
        }
        $this->http->fallback(reply(200, 'oops'));
        $e = $this->assertError('client.invalid-response', function () {
            $this->client->getTopic('q');
        });
        $this->assertTrue($e->isClientSide());
    }

    /// UC: UC-CNT-009, UC-CNT-017
    public function testATimeoutIsNotRepeatedAndIsAClientSideError()
    {
        $this->http->fallback(new NetworkFailure('timed out', false, true, false));
        $e = $this->assertError('client.timeout', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame(0, $e->status);
        $this->assertSame(1, count($this->http->requests));
        $this->http->fallback(new NetworkFailure('timed out', false, true, true));
        $this->assertError('client.network', function () {
            $this->client->getTopic('q');
        });
    }

    /// UC: UC-CNT-010
    public function testRetries429And503WaitingForRetryAfterThenBackoff()
    {
        $this->http->reply(
            reply(429, '{"code":"rate.limited","message":"slow"}', array('Retry-After' => '7')),
            reply(503, '{"code":"cluster.unavailable","message":"x"}', array('Retry-After' => '2')),
            reply(200, topicJson('q'))
        );
        $this->client->getTopic('q');
        $this->assertSame(3, count($this->http->requests));
        $this->assertSame(array(7.0, 2.0), $this->sleep->waits);
    }

    /// UC: UC-CNT-010
    public function testBackoffWithoutRetryAfterIsOneTwoFourSecondsUpToMaxAttempts()
    {
        $this->client->config()->maxAttempts = 4;
        $this->http->fallback(reply(503, '{"code":"cluster.unavailable","message":"down","requestId":"r9"}'));
        $e = $this->assertError('cluster.unavailable', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame(503, $e->status);
        $this->assertSame(4, count($this->http->requests));
        $this->assertSame(array(1.0, 2.0, 4.0), $this->sleep->waits);
    }

    /// UC: UC-CNT-011
    public function testAfterA503A409OnCreateAndA404OnDeleteCountAsSuccess()
    {
        $this->http->reply(
            reply(503, '{"code":"cluster.unavailable","message":"x"}'),
            reply(409, '{"code":"topic.already-exists","message":"x"}'),
            reply(200, topicJson('q')),
            reply(503, '{"code":"cluster.unavailable","message":"x"}'),
            reply(404, '{"code":"topic.not-found","message":"x"}')
        );
        $topic = $this->client->createTopic(new TopicRequest('q', 'queue'));
        $this->assertSame('q', $topic->name);
        $this->assertSame('GET', $this->http->requests[2]->method, 'the topic is read after the recovered 409');
        $this->client->deleteTopic('q');
        $this->assertSame(5, count($this->http->requests));
    }

    /// UC: UC-CNT-011, UC-CNT-014
    public function testWithoutAPrevious503A409OrA404IsAPlainError()
    {
        $this->http->reply(
            reply(409, '{"code":"topic.already-exists","message":"x"}'),
            reply(404, '{"code":"topic.not-found","message":"x"}')
        );
        $this->assertError('topic.already-exists', function () {
            $this->client->createTopic(new TopicRequest('q', 'queue'));
        });
        $this->assertError('topic.not-found', function () {
            $this->client->deleteTopic('q');
        });
        $this->assertSame(2, count($this->http->requests));
    }

    /// UC: UC-CNT-012
    public function testGatewayErrorsAreOnlyRepeatedWhenPublishingToAFifo()
    {
        $this->http->fallback(reply(502, '{"code":"cluster.bad-gateway","message":"x"}'));
        $this->assertError('cluster.bad-gateway', function () {
            $this->client->publish('q', '{}');
        });
        $this->assertSame(1, count($this->http->requests), 'a plain queue is not repeated');
        $this->assertError('cluster.bad-gateway', function () {
            $this->client->publish('fifo', '{}', array('fifo' => true));
        });
        $this->assertSame(4, count($this->http->requests), 'a FIFO is repeated up to the attempts');
        $this->http->fallback(reply(504, '{"code":"cluster.timeout","message":"x"}'));
        $this->assertError('cluster.timeout', function () {
            $this->client->consume('q');
        });
        $this->assertSame(5, count($this->http->requests), 'consume is not repeated');
    }

    /// UC: UC-CNT-013
    public function testANetworkFailureAfterSendingIsNotRepeated()
    {
        $this->http->fallback(new NetworkFailure('connection reset'));
        $this->assertError('client.network', function () {
            $this->client->publish('q', '{}');
        });
        $this->assertSame(1, count($this->http->requests));
    }

    /// UC: UC-CNT-013
    public function testARefusedConnectionIsRepeatedAndThenANetworkError()
    {
        $this->http->fallback(new NetworkFailure('refused', true));
        $e = $this->assertError('client.network', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame(0, $e->status);
        $this->assertSame(3, count($this->http->requests));
        $this->assertSame(array(1.0, 2.0), $this->sleep->waits);
    }

    /// UC: UC-CNT-013
    public function testARefusedConnectionSucceedsWhenTheNodeComesBack()
    {
        $this->http->reply(new NetworkFailure('refused', true), reply(200, topicJson('q')));
        $this->assertSame('q', $this->client->getTopic('q')->name);
        $this->assertSame(2, count($this->http->requests));
    }

    /// UC: UC-CNT-014
    public function testA4xxOtherThan429IsNeverRepeated()
    {
        foreach (array(400, 401, 403, 404, 409, 413, 422) as $status) {
            $before = count($this->http->requests);
            $this->http->reply(reply($status, '{"code":"x.y","message":"m"}'));
            $e = $this->assertError('x.y', function () {
                $this->client->getTopic('q');
            });
            $this->assertSame($status, $e->status);
            $this->assertSame($before + 1, count($this->http->requests));
        }
    }

    /// UC: UC-CNT-017, UC-CNT-018
    public function testTheErrorCarriesTheErrorResponseFieldsAndTheHeaders()
    {
        $this->client->config()->maxAttempts = 1;
        $this->http->reply(
            reply(429, '{"code":"rate.limited","message":"slow down","requestId":"body-id","details":{"limit":5}}', array('Retry-After' => '9')),
            reply(500, '{"code":"internal.error","message":"boom"}', array('X-Request-Id' => 'header-id'))
        );
        $e = $this->assertError('rate.limited', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame(429, $e->status);
        $this->assertSame('slow down', $e->getMessage());
        $this->assertSame('body-id', $e->requestId);
        $this->assertSame(9, $e->retryAfter);
        $this->assertSame('{"limit":5}', $e->details);
        $this->assertFalse($e->isClientSide());
        $this->assertContains('body-id', (string) $e);
        $e = $this->assertError('internal.error', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame('header-id', $e->requestId);
        $this->assertNull($e->retryAfter);
    }

    /// UC: UC-CNT-017
    public function testANonJsonErrorStillReportsAStatusCode()
    {
        $this->http->reply(reply(400, '<html>bad</html>'), reply(401, ''));
        $e = $this->assertError('http.400', function () {
            $this->client->getTopic('q');
        });
        $this->assertSame('<html>bad</html>', $e->getMessage());
        $e = $this->assertError('http.401', function () {
            $this->client->getTopic('q');
        });
        $this->assertContains('401', $e->getMessage());
    }

    /// UC: UC-CNT-031
    public function testSubscribeAcksOnlyWhatTheHandlerAcceptedAndStopsOnCancel()
    {
        $this->http->batches(array(messageJson('m1', 'a'), messageJson('m2', 'b'), messageJson('m3', 'c')));
        $cancel = new CancelToken();
        $seen = array();
        $failed = array();
        $this->client->subscribe('q', function (Message $m) use (&$seen, $cancel) {
            $seen[] = $m->id;
            if ($m->id === 'm3') {
                $cancel->cancel();
            }
            if ($m->id === 'm2') {
                throw new \RuntimeException('the handler failed');
            }
        }, array('visibilityTimeout' => 60), $cancel, function (Message $m, \Exception $e) use (&$failed) {
            $failed[] = $m->id . ':' . $e->getMessage();
        });
        $this->assertSame(array('m1', 'm2', 'm3'), $seen);
        $this->assertSame(array('m2:the handler failed'), $failed);
        $deleted = array();
        foreach ($this->http->requests as $r) {
            if ($r->method === 'DELETE') {
                $deleted[] = $r->path;
            }
        }
        $this->assertSame(array('/topics/q/messages/m1', '/topics/q/messages/m3'), $deleted, 'only the accepted ones are acked');
        $first = $this->http->requests[0];
        $this->assertSame('20', $first->query['waitTime']);
        $this->assertSame('1', $first->query['maxMessages']);
        $this->assertSame('60', $first->query['visibilityTimeout']);
    }

    /// UC: UC-CNT-031
    public function testSubscribeReturnsWithoutAnErrorWhenCancelledDuringTheLongPolling()
    {
        $cancel = new CancelToken();
        $this->http->fallback(function () use ($cancel) {
            $cancel->cancel();
            return new \Taba\NetworkFailure('aborted');
        });
        $executor = $this->http;
        $client = new Client(new Config('http://taba.test'), $this->sleep, function ($url, $method, $headers, $body, $timeout, $config, $token) use ($executor, $cancel) {
            if ($token->isCancelled()) {
                throw new \Taba\Cancelled();
            }
            return call_user_func($executor, $url, $method, $headers, $body, $timeout, $config, $token);
        });
        $cancel->cancel();
        $client->subscribe('q', function () {
        }, null, $cancel);
        $this->assertSame(0, count($this->http->requests));
        // A server error while consuming ends the loop with the error.
        $this->http->fallback(reply(403, '{"code":"auth.forbidden","message":"no"}'));
        $this->assertError('auth.forbidden', function () {
            $this->client->subscribe('q', function () {
            });
        });
    }

    /// UC: UC-CNT-033
    public function testPublishJsonSerializesTheValueAndSendsItAsJson()
    {
        $this->client->publishJson('q', array('order' => 9001, 'note' => '<ok> & é / x'));
        $sent = $this->http->last();
        $this->assertSame('application/json', $sent->header('Content-Type'));
        $this->assertSame('{"order":9001,"note":"<ok> & é / x"}', $sent->body);
        $this->client->publishRawJson('q', '{"a": 1}', array('groupId' => 'g'));
        $this->assertSame('{"a": 1}', $this->http->last()->body);
        $this->assertSame('g', $this->http->last()->query['groupId']);
        $this->assertError('client.invalid-request', function () {
            $this->client->publishJson('q', null);
        });
        $this->assertError('client.invalid-request', function () {
            $this->client->publishJson('q', "\xff\xfe");
        });
        $this->assertSame(2, count($this->http->requests), 'nothing was sent for the invalid ones');
    }

    private function deletes()
    {
        $n = 0;
        foreach ($this->http->requests as $r) {
            if ($r->method === 'DELETE') {
                $n++;
            }
        }
        return $n;
    }

    /// UC: UC-CNT-034
    public function testConsumeWithAHandlerDeletesByDefaultAndKeepsTheMessageWhenAskedOrWhenItFails()
    {
        $this->http->batches(array(messageJson('m1', array('n' => 1))), array(messageJson('m2', 'x')), array(messageJson('m3', 'y')));
        $seen = array();
        $handled = $this->client->consumeWith('q', function (MessageContext $c) use (&$seen) {
            $seen[] = $c->message->json();
        });
        $this->assertSame(1, $handled);
        $this->assertSame(array(array('n' => 1)), $seen);
        $this->assertSame(1, $this->deletes());
        $first = $this->http->requests[0];
        $this->assertSame('1', $first->query['maxMessages']);
        $this->assertSame('120', $first->query['visibilityTimeout']);
        $this->assertSame('20', $first->query['waitTime']);

        $this->client->consumeWith('q', function (MessageContext $c) {
            $c->delete = false;
        });
        $this->assertSame(1, $this->deletes(), 'delete = false keeps the message');

        $e = $this->assertThrows('RuntimeException', function () {
            $this->client->consumeWith('q', function () {
                throw new \RuntimeException('boom');
            });
        });
        $this->assertSame('boom', $e->getMessage());
        $this->assertSame(1, $this->deletes(), 'a failing handler keeps the message');
    }

    /// UC: UC-CNT-034
    public function testTheHandlerCanChangeTheVisibilityAndATolerated404ofTheDelete()
    {
        $this->http->fallback(function (Recorded $r) {
            if ($r->method === 'GET') {
                return reply(200, array('messages' => array(messageJson('m1', 'a'))));
            }
            if ($r->method === 'PUT') {
                return reply(200, array('id' => 'm1', 'invisibleUntil' => 'later'));
            }
            return reply(404, '{"code":"message.not-found","message":"gone"}');
        });
        $handled = $this->client->consumeWith('q', function (MessageContext $c) {
            $this->assertSame(30, $c->leaseSeconds());
            $c->changeVisibility(60);
            $this->assertSame(60, $c->leaseSeconds());
        }, array('visibilityTimeout' => 30));
        $this->assertSame(1, $handled, 'the 404 of the delete is tolerated');
        $put = $this->http->requests[1];
        $this->assertSame('PUT', $put->method);
        $this->assertSame('{"visibilityTimeout":60}', $put->body);
        $this->assertSame('rt-m1', $put->query['receiptToken']);
    }

    /// UC: UC-CNT-034
    public function testConsumeJsonHandsOverTheParsedBodyAndRefusesABodyThatIsNotJson()
    {
        $this->http->batches(
            array(messageJson('m1', array('order' => 7, 'items' => array(1, 2)))),
            array(messageJson('m2', 'plain text', array('contentType' => 'text/plain; charset=utf-8')))
        );
        $got = null;
        $this->client->consumeJson('q', function (MessageContext $c) use (&$got) {
            $got = $c->body;
        });
        $this->assertSame(array('order' => 7, 'items' => array(1, 2)), $got);
        $before = $this->deletes();
        $this->assertError('client.invalid-response', function () {
            $this->client->consumeJson('q', function () {
                $this->fail('the handler must not run');
            });
        });
        $this->assertSame($before, $this->deletes(), 'the message is not deleted');
    }
}
