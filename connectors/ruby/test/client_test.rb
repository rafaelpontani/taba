require_relative 'support'

class OperationsTest < ClientCase
  # UC: UC-CNT-001, UC-CNT-029
  def test_the_nine_operations_build_the_expected_routes
    @server.fallback(reply(200, topic_json('a b')))
    @client.create_topic(Taba::TopicRequest.new(name: 'a b', type: 'queue'))
    @client.get_topic('a b')
    @client.list_topics(limit: 5, cursor: 'cur', name: 'ord')
    @client.update_topic('a b', idle_timeout: 9)
    @client.delete_topic('a b')
    @client.publish('a b', '{}')
    @client.consume('a b')
    @client.ack('a b', 'm 1', 'r/1')
    @client.change_visibility('a b', 'm 1', 30, 'r/1')

    expected = [
      ['POST', '/topics'], ['GET', '/topics/a%20b'], ['GET', '/topics'], ['PATCH', '/topics/a%20b'],
      ['DELETE', '/topics/a%20b'], ['POST', '/topics/a%20b/messages'], ['GET', '/topics/a%20b/messages'],
      ['DELETE', '/topics/a%20b/messages/m%201'], ['PUT', '/topics/a%20b/messages/m%201/visibility']
    ]
    r = @server.all
    assert_equal expected, r.map { |x| [x.method, x.path] }
    assert_equal '5', r[2].query['limit']
    assert_equal 'cur', r[2].query['cursor']
    assert_equal 'ord', r[2].query['name']
    assert_equal 'r/1', r[7].query['receiptToken']
    assert_equal '{"idleTimeout":9}', r[3].text
    assert_equal '{"visibilityTimeout":30}', r[8].text
    r.each { |x| assert_nil x.headers['x-http-method-override'], 'a verb is never overridden' }
  end

  # UC: UC-CNT-001
  def test_create_topic_sends_only_what_was_filled
    @server.fallback(reply(201, topic_json('q')))
    @client.create_topic(name: 'q', type: 'queue')
    assert_equal '{"name":"q","type":"queue"}', @server.all.last.text

    @client.create_topic(name: 'd', type: 'distributor', prefixes: %w(a- b-), tags: { 'k' => 'v' }, fifo: true, ttl: 60,
                         max_deliveries: 3, volatile: false, idle_timeout: 5, max_message_bytes: 1024)
    body = JSON.parse(@server.all.last.text)
    %w(prefixes tags fifo ttl maxDeliveries volatile idleTimeout maxMessageBytes).each do |key|
      assert body.key?(key), key
    end
    assert_equal false, body['volatile']

    # PATCH: a key left out is not sent, nil is a JSON null (back to the server default).
    @client.update_topic('q', ttl: nil, max_deliveries: 4, tags: {})
    assert_equal({ 'ttl' => nil, 'maxDeliveries' => 4, 'tags' => {} }, JSON.parse(@server.all.last.text))
    @client.update_topic('q')
    assert_equal '{}', @server.all.last.text
    assert_error('client.invalid-request') { Taba::TopicRequest.new(typo: 1) }
  end

  # UC: UC-CNT-018
  def test_headers_carry_the_agent_and_the_optional_ids
    @client.publish('q', 'x', request_id: 'req-1', traceparent: '00-aa-bb-01')
    @client.consume('q', request_id: 'req-2')
    r = @server.all
    assert_equal 'req-1', r[0].headers['x-request-id']
    assert_equal '00-aa-bb-01', r[0].headers['traceparent']
    assert_equal 'application/json', r[0].headers['accept']
    assert_match(/\Ataba-ruby\//, r[0].headers['user-agent'])
    assert_equal 'req-2', r[1].headers['x-request-id']
    assert_nil r[1].headers['traceparent']
  end

  # UC: UC-CNT-004, UC-CNT-025
  def test_publish_sends_raw_bytes_with_content_type_and_query
    @server.fallback(reply(201, 'id' => 'm1', 'duplicate' => true))
    payload = [0, 1, 2, 255].pack('C*')
    r = @client.publish('q', payload, content_type: 'application/octet-stream', group_id: 'g 1', deduplication_id: 'd1')
    assert_equal 'm1', r.id
    assert_equal true, r.duplicate
    sent = @server.all.last
    assert_equal payload.bytes, sent.body.bytes
    assert_equal 'application/octet-stream', sent.headers['content-type']
    assert_equal 'g 1', sent.query['groupId']
    assert_equal 'd1', sent.query['deduplicationId']
    # No content type: application/json. The connector does not validate the body.
    @client.publish('q', 'not json')
    assert_equal 'application/json', @server.all.last.headers['content-type']
    @client.publish_text('q', 'olá')
    assert_equal 'text/plain; charset=utf-8', @server.all.last.headers['content-type']
    assert_equal 'olá', @server.all.last.text
  end

  # UC: UC-CNT-041
  def test_the_batches_send_one_json_and_read_one_result_per_item
    @server.reply(
      reply(200, 'results' => [
        { 'status' => 201, 'id' => 'a' }, { 'status' => 200, 'id' => 'b', 'duplicate' => true },
        { 'status' => 400, 'error' => { 'code' => 'message.invalid-content-type', 'message' => 'bad' } }
      ]),
      reply(200, 'results' => [
        { 'status' => 204 }, { 'status' => 409, 'error' => { 'code' => 'message.receipt-invalid', 'message' => 'old' } }
      ]),
      reply(200, 'results' => [{ 'status' => 200, 'id' => 'm1', 'invisibleUntil' => '2026-10-27T10:00:30Z' }])
    )
    results = @client.publish_batch('q', [
      Taba::BatchMessage.new('one', content_type: 'text/plain; charset=utf-8', group_id: 'g'),
      Taba::BatchMessage.new([0, 255].pack('C*')),
      Taba::BatchMessage.new('x', content_type: 'not a type')
    ])
    assert_equal 3, results.size
    assert results[0].ok?
    assert_equal true, results[1].duplicate
    refute results[2].ok?, 'the third fails alone'
    assert_equal 'message.invalid-content-type', results[2].error_code

    sent = @server.all[0]
    assert_equal 'POST', sent.method
    assert_equal '/topics/q/messages/batch', sent.path
    items = JSON.parse(sent.text)['messages']
    assert_equal 'b25l', items[0]['body']
    assert_equal 'base64', items[0]['bodyEncoding']
    assert_equal 'g', items[0]['groupId']
    refute items[1].key?('contentType'), 'a content type nobody set is not sent'

    messages = [Taba::Message.new(id: 'm1', receipt_token: 'r1'), Taba::Message.new(id: 'm2', receipt_token: 'r2')]
    acked = @client.ack_batch('q', messages)
    assert_equal 204, acked[0].status
    assert_equal 'message.receipt-invalid', acked[1].error_code
    assert_equal '/topics/q/messages/ack', @server.all[1].path
    assert_equal '{"messages":[{"id":"m1","receiptToken":"r1"},{"id":"m2","receiptToken":"r2"}]}', @server.all[1].text

    visible = @client.change_visibility_batch('q', [messages[0]], 30)
    assert_equal '2026-10-27T10:00:30Z', visible[0].invisible_until
    assert_equal '/topics/q/messages/visibility', @server.all[2].path
    assert_equal '{"messages":[{"id":"m1","receiptToken":"r1","visibilityTimeout":30}]}', @server.all[2].text
  end

  # UC: UC-CNT-004
  def test_a_distributor_answer_lists_each_copy_and_the_partial_failures
    @server.fallback(reply(200, '{"matched":2,"deliveries":[{"topic":"a-1","id":"x"},{"topic":"a-2","error":{"code":"queue.full","message":"full"}}]}'))
    r = @client.publish('d', '{}')
    assert_equal 2, r.matched
    assert_nil r.id
    assert r.deliveries[0].succeeded?
    refute r.deliveries[1].succeeded?
    assert_equal 'queue.full', r.deliveries[1].error_code
  end

  # UC: UC-CNT-002
  def test_config_defaults_and_required_base_url
    [nil, '', '  ', 'localhost:8080', 'ftp://x', 'http://'].each do |bad|
      assert_error('client.invalid-config') { Taba::Client.new(bad) }
    end
    config = Taba::Config.new('http://localhost:8080')
    assert_equal 5.0, config.connect_timeout
    assert_equal 30.0, config.request_timeout
    assert_equal 3, config.max_attempts
    assert_equal "taba-ruby/#{Taba::VERSION}", config.user_agent
    assert_equal 't', Taba::Client.new('http://x', token: 't').config.token
    assert_error('client.invalid-config') { Taba::Client.new('http://x', tokn: 't') }
  end

  # UC: UC-CNT-003
  def test_a_token_goes_in_every_call_and_without_it_no_call_carries_authorization
    with = Taba::Client.new(@server.url, token: 'tb_x.secret')
    with.get_topic('a')
    with.consume('a')
    @server.all.each { |r| assert_equal 'Bearer tb_x.secret', r.headers['authorization'] }
    with.close
    count = @server.count
    @client.get_topic('a')
    assert_nil @server.all[count].headers['authorization']
  end

  # UC: UC-CNT-028
  def test_unknown_fields_in_responses_are_ignored_and_the_contract_version_is_declared
    @server.fallback(reply(200, '{"id":"t1","name":"q","type":"queue","createdAt":"x","somethingNew":{"a":[1,2]},"fifo":false}'))
    topic = @client.get_topic('q')
    assert_equal 'q', topic.name
    assert_equal false, topic.fifo
    assert_nil topic.volatile
    assert_equal '1', Taba::CONTRACT_VERSION
  end
end

class ConsumingTest < ClientCase
  # UC: UC-CNT-005
  def test_consume_sends_only_what_was_given
    @client.consume('q')
    @client.consume('q', max_messages: 5, visibility_timeout: 60, wait_time: 20)
    r = @server.all
    assert_nil r[0].raw_query, 'no defaults of its own'
    assert_equal '5', r[1].query['maxMessages']
    assert_equal '60', r[1].query['visibilityTimeout']
    assert_equal '20', r[1].query['waitTime']
  end

  # UC: UC-CNT-005
  def test_auto_create_parameters_are_left_out_without_auto_create
    @client.consume('q', volatile: true, fifo: true, tags: { 'a' => 'b' })
    @client.consume('q', auto_create: true, volatile: true, idle_timeout: 30, fifo: false, tags: { 'a' => 'b' })
    r = @server.all
    assert_nil r[0].raw_query
    assert_equal 'true', r[1].query['autoCreate']
    assert_equal 'true', r[1].query['volatile']
    assert_equal '30', r[1].query['idleTimeout']
    assert_equal 'false', r[1].query['fifo']
    assert_equal 'b', r[1].query['tag.a']
  end

  # UC: UC-CNT-008
  def test_the_consume_read_timeout_is_wait_time_plus_ten_seconds_and_not_the_request_timeout
    assert_equal 30.0, Taba::Calls.consume('q', Taba::ConsumeOptions.new(wait_time: 20)).timeout
    assert_equal 10.0, Taba::Calls.consume('q', Taba::ConsumeOptions.new).timeout
    client = Taba::Client.new(@server.url, request_timeout: 0.1)
    @server.fallback do |_request|
      sleep 0.4
      reply(200, 'messages' => [])
    end
    # The ordinary timeout would cut this at 0.1 s; the long polling has wait_time + 10 s.
    assert_equal [], client.consume('q', wait_time: 1)
    client.close
  end

  # UC: UC-CNT-006
  def test_ack_and_visibility_put_the_receipt_token_in_the_query
    @server.reply(reply(204, ''), reply(200, 'id' => 'm1', 'invisibleUntil' => 'later'))
    m = Taba::Message.new(id: 'm1', receipt_token: 'a/b c')
    @client.ack('q', m)
    v = @client.change_visibility('q', m, 45)
    assert_equal 'a/b c', @server.all[0].query['receiptToken']
    assert_equal 'a/b c', @server.all[1].query['receiptToken']
    assert_equal 'later', v.invisible_until
    assert_error('client.invalid-request') { @client.ack('q', 'm1') }
  end

  # UC: UC-CNT-007
  def test_list_all_topics_follows_next_cursor_until_the_last_page
    @server.reply(
      reply(200, 'items' => [topic_json('a'), topic_json('b')], 'nextCursor' => 'c1'),
      reply(200, 'items' => [topic_json('c')], 'nextCursor' => nil)
    )
    assert_equal %w(a b c), @client.list_all_topics.map(&:name)
    assert_nil @server.all[0].raw_query
    assert_equal 'c1', @server.all[1].query['cursor']
  end

  # UC: UC-CNT-015, UC-CNT-016, UC-CNT-025, UC-CNT-026
  def test_messages_come_back_as_bytes_for_json_text_and_base64
    @server.fallback(reply(200, <<-JSON))
      {"messages":[
        {"id":"m1","body":{"a": 1.0,"b":"\\u00e9"},"contentType":"application/json","receiptToken":"r1","deliveryCount":2,"groupId":"g","deduplicationId":"d","extra":1},
        {"id":"m2","body":"caf\\u00e9","contentType":"text/plain; charset=utf-8","receiptToken":"r2"},
        {"id":"m3","body":"AAEC/w==","bodyEncoding":"base64","contentType":"application/octet-stream","receiptToken":"r3"},
        {"id":"m4","body":[1,2],"contentType":"application/vnd.x+json","receiptToken":"r4","deadLetteredAt":"t","deadLetterReason":"max"}
      ]}
    JSON
    messages = @client.consume('q')
    # JSON is the text of the field as it came, not re-formatted.
    assert_equal '{"a": 1.0,"b":"\u00e9"}', messages[0].body_text
    assert_equal 2, messages[0].delivery_count
    assert_equal 'g', messages[0].group_id
    assert_equal 'café', messages[1].body_text
    assert_equal [0, 1, 2, 255], messages[2].body.bytes
    assert_equal '[1,2]', messages[3].body_text
    assert_equal 'max', messages[3].dead_letter_reason
    assert_equal 1.0, messages[0].json['a']
  end

  # UC: UC-CNT-026
  def test_an_invalid_answer_is_an_invalid_response_error_and_not_a_crash
    ['not json', '[]', '{"messages":[{"id":5}]}', '{"messages":[{"id":"m","body":"!!","bodyEncoding":"base64"}]}', '{"messages":{}}'].each do |body|
      @server.fallback(reply(200, body))
      assert_error('client.invalid-response') { @client.consume('q') }
    end
    @server.fallback(reply(200, 'oops'))
    error = assert_error('client.invalid-response') { @client.get_topic('q') }
    assert error.client_side?
  end
end

class RetryTest < ClientCase
  # UC: UC-CNT-009, UC-CNT-017
  def test_a_timeout_is_not_repeated_and_is_a_client_side_error
    client = Taba::Client.new(@server.url, request_timeout: 0.2)
    client.sleeper = @sleep
    @server.fallback(ScriptedServer::HANG)
    error = assert_error('client.timeout') { client.get_topic('q') }
    assert_equal 0, error.status
    assert_equal 1, @server.count
    client.close
  end

  # UC: UC-CNT-010
  def test_retries_429_and_503_waiting_for_retry_after_then_backoff
    @server.reply(
      reply(429, '{"code":"rate.limited","message":"slow"}', 'Retry-After' => '7'),
      reply(503, '{"code":"cluster.unavailable","message":"x"}', 'Retry-After' => '2'),
      reply(200, topic_json('q'))
    )
    @client.get_topic('q')
    assert_equal 3, @server.count
    assert_equal [7.0, 2.0], @sleep.waits
  end

  # UC: UC-CNT-010
  def test_backoff_without_retry_after_is_one_two_four_seconds_up_to_max_attempts
    client = Taba::Client.new(@server.url, max_attempts: 4)
    client.sleeper = @sleep
    @server.fallback(reply(503, '{"code":"cluster.unavailable","message":"down","requestId":"r9"}'))
    error = assert_error('cluster.unavailable') { client.get_topic('q') }
    assert_equal 503, error.status
    assert_equal 4, @server.count
    assert_equal [1.0, 2.0, 4.0], @sleep.waits
    client.close
  end

  # UC: UC-CNT-011
  def test_after_a_503_a_409_on_create_and_a_404_on_delete_count_as_success
    @server.reply(
      reply(503, '{"code":"cluster.unavailable","message":"x"}'),
      reply(409, '{"code":"topic.already-exists","message":"x"}'),
      reply(200, topic_json('q')),
      reply(503, '{"code":"cluster.unavailable","message":"x"}'),
      reply(404, '{"code":"topic.not-found","message":"x"}')
    )
    topic = @client.create_topic(name: 'q', type: 'queue')
    assert_equal 'q', topic.name
    assert_equal 'GET', @server.all[2].method, 'the topic is read after the recovered 409'
    @client.delete_topic('q')
    assert_equal 5, @server.count
  end

  # UC: UC-CNT-011, UC-CNT-014
  def test_without_a_previous_503_a_409_or_a_404_is_a_plain_error
    @server.reply(
      reply(409, '{"code":"topic.already-exists","message":"x"}'),
      reply(404, '{"code":"topic.not-found","message":"x"}')
    )
    assert_error('topic.already-exists') { @client.create_topic(name: 'q', type: 'queue') }
    assert_error('topic.not-found') { @client.delete_topic('q') }
    assert_equal 2, @server.count
  end

  # UC: UC-CNT-012
  def test_gateway_errors_are_only_repeated_when_publishing_to_a_fifo
    @server.fallback(reply(502, '{"code":"cluster.bad-gateway","message":"x"}'))
    assert_error('cluster.bad-gateway') { @client.publish('q', '{}') }
    assert_equal 1, @server.count, 'a plain queue is not repeated'
    assert_error('cluster.bad-gateway') { @client.publish('fifo', '{}', fifo: true) }
    assert_equal 4, @server.count, 'a FIFO is repeated up to the attempts'
    @server.fallback(reply(504, '{"code":"cluster.timeout","message":"x"}'))
    assert_error('cluster.timeout') { @client.consume('q') }
    assert_equal 5, @server.count, 'consume is not repeated'
  end

  # UC: UC-CNT-013
  def test_a_network_failure_after_sending_is_not_repeated
    @server.fallback(ScriptedServer::DESTROY)
    assert_error('client.network') { @client.publish('q', '{}') }
    assert_equal 1, @server.count
  end

  def closed_port
    server = TCPServer.new('127.0.0.1', 0)
    port = server.addr[1]
    server.close
    port
  end

  # UC: UC-CNT-013
  def test_a_refused_connection_is_repeated_and_then_a_network_error
    client = Taba::Client.new("http://127.0.0.1:#{closed_port}")
    client.sleeper = @sleep
    error = assert_error('client.network') { client.get_topic('q') }
    assert_equal 0, error.status
    assert_equal [1.0, 2.0], @sleep.waits
    client.close
  end

  # UC: UC-CNT-013
  def test_a_refused_connection_succeeds_when_the_node_comes_back
    port = closed_port
    client = Taba::Client.new("http://127.0.0.1:#{port}")
    server = nil
    client.sleeper = lambda do |_seconds, _token|
      server ||= ScriptedServer.new(port).fallback(reply(200, topic_json('q')))
    end
    assert_equal 'q', client.get_topic('q').name
    client.close
    server.close if server
  end

  # UC: UC-CNT-014
  def test_a_4xx_other_than_429_is_never_repeated
    [400, 401, 403, 404, 409, 413, 422].each do |status|
      before = @server.count
      @server.reply(reply(status, '{"code":"x.y","message":"m"}'))
      error = assert_error('x.y') { @client.get_topic('q') }
      assert_equal status, error.status
      assert_equal before + 1, @server.count
    end
  end

  # UC: UC-CNT-017, UC-CNT-018
  def test_the_error_carries_the_error_response_fields_and_the_headers
    client = Taba::Client.new(@server.url, max_attempts: 1)
    @server.reply(
      reply(429, '{"code":"rate.limited","message":"slow down","requestId":"body-id","details":{"limit":5}}', 'Retry-After' => '9'),
      reply(500, '{"code":"internal.error","message":"boom"}', 'X-Request-Id' => 'header-id')
    )
    error = assert_error('rate.limited') { client.get_topic('q') }
    assert_equal 429, error.status
    assert_equal 'slow down', error.reason
    assert_equal 'body-id', error.request_id
    assert_equal 9, error.retry_after
    assert_equal '{"limit":5}', error.details
    refute error.client_side?
    assert_includes error.to_s, 'body-id'
    error = assert_error('internal.error') { client.get_topic('q') }
    assert_equal 'header-id', error.request_id
    assert_nil error.retry_after
    client.close
  end

  # UC: UC-CNT-017
  def test_a_non_json_error_still_reports_a_status_code
    @server.reply(reply(400, '<html>bad</html>'), reply(401, ''))
    error = assert_error('http.400') { @client.get_topic('q') }
    assert_equal '<html>bad</html>', error.reason
    error = assert_error('http.401') { @client.get_topic('q') }
    assert_includes error.reason, '401'
  end

  # UC: UC-CNT-020
  def test_many_threads_can_use_one_client_at_the_same_time
    @server.fallback(reply(200, topic_json('q')))
    results = Queue.new
    threads = Array.new(20) do
      Thread.new do
        results << @client.get_topic('q').name
        @client.close
      end
    end
    threads.each(&:join)
    assert_equal 20, results.size
    assert_equal 20, @server.count
  end

  # UC: UC-CNT-020
  def test_cancelling_the_token_cancels_the_call_without_a_client_error
    @server.fallback(ScriptedServer::HANG)
    token = Taba::CancelToken.new
    Thread.new { sleep 0.2; token.cancel }
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    assert_raises(Taba::Cancelled) { @client.consume('q', wait_time: 20, cancel: token) }
    assert_operator Process.clock_gettime(Process::CLOCK_MONOTONIC) - started, :<, 5, 'the long polling waited for its timeout'
    # Already cancelled: no request goes out.
    count = @server.count
    assert_raises(Taba::Cancelled) { @client.get_topic('q', cancel: token) }
    assert_equal count, @server.count
  end
end

class HandlerTest < ClientCase
  def deletes
    @server.all.select { |r| r.method == 'DELETE' }
  end

  # UC: UC-CNT-031
  def test_subscribe_acks_only_what_the_handler_accepted_and_stops_on_cancel
    @server.batches([message_json('m1', 'a'), message_json('m2', 'b'), message_json('m3', 'c')])
    cancel = Taba::CancelToken.new
    seen = []
    failed = []
    @client.subscribe('q', cancel: cancel, visibility_timeout: 60,
                      on_error: lambda { |m, e| failed << "#{m.id}:#{e.message}" }) do |m|
      seen << m.id
      cancel.cancel if m.id == 'm3'
      raise 'the handler failed' if m.id == 'm2'
    end
    assert_equal %w(m1 m2 m3), seen
    assert_equal ['m2:the handler failed'], failed
    assert_equal ['/topics/q/messages/m1', '/topics/q/messages/m3'], deletes.map(&:path), 'only the accepted ones are acked'
    first = @server.all[0]
    assert_equal '20', first.query['waitTime']
    assert_equal '1', first.query['maxMessages']
    assert_equal '60', first.query['visibilityTimeout']
  end

  # UC: UC-CNT-031
  def test_subscribe_returns_without_an_error_when_cancelled_during_the_long_polling
    @server.fallback(ScriptedServer::HANG)
    cancel = Taba::CancelToken.new
    Thread.new { sleep 0.2; cancel.cancel }
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    @client.subscribe('q', cancel: cancel) { |_m| }
    assert_operator Process.clock_gettime(Process::CLOCK_MONOTONIC) - started, :<, 5
    # A server error while consuming ends the loop with the error.
    @server.fallback(reply(403, '{"code":"auth.forbidden","message":"no"}'))
    assert_error('auth.forbidden') { @client.subscribe('q') { |_m| } }
  end

  # UC: UC-CNT-033
  def test_publish_json_serializes_the_value_and_sends_it_as_json
    @client.publish_json('q', order: 9001, note: '<ok> & é / x')
    sent = @server.all.last
    assert_equal 'application/json', sent.headers['content-type']
    assert_equal({ 'order' => 9001, 'note' => '<ok> & é / x' }, JSON.parse(sent.text))
    @client.publish_json('q', 'just a string')
    assert_equal '"just a string"', @server.all.last.text
    @client.publish_raw_json('q', '{"a": 1}', group_id: 'g')
    assert_equal '{"a": 1}', @server.all.last.text
    assert_equal 'g', @server.all.last.query['groupId']
    assert_error('client.invalid-request') { @client.publish_json('q', nil) }
    assert_error('client.invalid-request') { @client.publish_json('q', "\xff\xfe".force_encoding('UTF-8')) }
    assert_equal 3, @server.count, 'nothing was sent for the invalid ones'
  end

  # UC: UC-CNT-034
  def test_consume_with_a_handler_deletes_by_default_and_keeps_the_message_when_asked_or_when_it_fails
    @client.sleeper = BLOCKING_SLEEP
    @server.batches([message_json('m1', 'n' => 1)], [message_json('m2', 'x')], [message_json('m3', 'y')])
    seen = []
    handled = @client.consume_with('q') { |c| seen << c.message.json }
    assert_equal 1, handled
    assert_equal [{ 'n' => 1 }], seen
    assert_equal 1, deletes.size
    first = @server.all[0]
    assert_equal '1', first.query['maxMessages']
    assert_equal '120', first.query['visibilityTimeout']
    assert_equal '20', first.query['waitTime']

    @client.consume_with('q') { |c| c.delete = false }
    assert_equal 1, deletes.size, 'delete = false keeps the message'

    error = assert_raises(RuntimeError) { @client.consume_with('q') { |_c| raise 'boom' } }
    assert_equal 'boom', error.message
    assert_equal 1, deletes.size, 'a failing handler keeps the message'
  end

  # UC: UC-CNT-034
  def test_consume_with_a_handler_renews_the_visibility_while_it_runs_and_lets_it_change_it
    calls = 0
    interval = nil
    mutex = Mutex.new
    @client.sleeper = lambda do |seconds, token|
      n = mutex.synchronize { calls += 1 }
      interval = seconds if n == 1
      raise Taba::Cancelled if n > 1 && token.wait(60)
    end
    @server.batches([message_json('m1', 'a')])
    puts = lambda { @server.all.select { |r| r.method == 'PUT' } }
    @client.consume_with('q', visibility_timeout: 30) do |c|
      wait_for { puts.call.size >= 1 }
      assert_equal 30, c.lease_seconds
      c.change_visibility(60)
      assert_equal 60, c.lease_seconds
    end
    assert_in_delta 10.0, interval, 0.001, 'a third of the invisibility'
    p = puts.call
    assert_equal '{"visibilityTimeout":30}', p[0].text
    assert_equal '{"visibilityTimeout":60}', p[1].text
    assert_equal 'rt-m1', p[0].query['receiptToken']
  end

  # UC: UC-CNT-034
  def test_a_refused_renewal_marks_the_lease_as_lost_and_the_delete_is_tolerated
    count = 0
    mutex = Mutex.new
    @client.sleeper = lambda do |_seconds, token|
      n = mutex.synchronize { count += 1 }
      raise Taba::Cancelled if n > 1 && token.wait(60)
    end
    @server.fallback do |request|
      case request.method
      when 'GET' then reply(200, 'messages' => [message_json('m1', 'a')])
      when 'PUT' then reply(409, '{"code":"message.receipt-invalid","message":"gone"}')
      else reply(404, '{"code":"message.not-found","message":"gone"}')
      end
    end
    lost = nil
    handled = @client.consume_with('q') do |c|
      wait_for { c.lease_lost? }
      lost = c.lease_lost?
    end
    assert_equal 1, handled, 'the 404 of the delete must be tolerated'
    assert_equal true, lost
  end

  # UC: UC-CNT-034
  def test_consume_json_hands_over_the_parsed_body_and_refuses_a_body_that_is_not_json
    @client.sleeper = BLOCKING_SLEEP
    @server.batches(
      [message_json('m1', 'order' => 7, 'items' => [1, 2])],
      [message_json('m2', 'plain text', 'contentType' => 'text/plain; charset=utf-8')]
    )
    got = nil
    @client.consume_json('q') { |c| got = c.body }
    assert_equal({ 'order' => 7, 'items' => [1, 2] }, got)
    before = deletes.size
    assert_error('client.invalid-response') { @client.consume_json('q') { |_c| flunk 'the handler must not run' } }
    assert_equal before, deletes.size, 'the message is not deleted'
  end
end
