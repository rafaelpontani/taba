require 'json'
require 'thread'
require 'taba/calls'
require 'taba/cancel_token'
require 'taba/config'
require 'taba/error'
require 'taba/models'
require 'taba/options'
require 'taba/transport'

module Taba
  # What the +consume_with+ handler receives: the +message+, the +delete+ flag (the message is deleted at the end if the
  # handler returns without raising; the default is true) and +change_visibility+. While the handler runs, the
  # connector renews the invisibility by itself, so no other consumer takes the message.
  class MessageContext
    attr_reader :message
    # Set it to false to keep the message even though the handler returned normally.
    attr_accessor :delete
    # +consume_json+ only: the parsed body.
    attr_accessor :body

    def initialize(message, lease_seconds, change)
      @message = message
      @delete = true
      @body = nil
      @mutex = Mutex.new
      @lease_seconds = lease_seconds
      @lease_lost = false
      @change = change
    end

    # The seconds of invisibility the automatic renewal requests on each round.
    def lease_seconds
      @mutex.synchronize { @lease_seconds }
    end

    # True when the server refused the renewal (the message already went back to the queue or was deleted).
    def lease_lost?
      @mutex.synchronize { @lease_lost }
    end

    # Changes the invisibility to +seconds+ from now on. The value becomes the one of the automatic renewal.
    def change_visibility(seconds)
      @change.call(seconds)
      @mutex.synchronize { @lease_seconds = seconds }
      nil
    end

    # :nodoc:
    def lease_lost!
      @mutex.synchronize { @lease_lost = true }
    end
  end

  # The Taba client API client (doc 21): the nine operations, +subscribe+ and the consume with a handler. It is safe
  # for concurrent use: each thread has its own connection.
  #
  #   client = Taba::Client.new('http://localhost:8080', token: token)
  #   client.publish_json('orders', order: 9001)
  #   client.consume_with('orders', wait_time: 20) { |context| puts context.message.body_text }
  class Client
    # Creates the client from a Taba::Config, or from the address and the Config fields as options.
    def initialize(config = nil, **options)
      if config.is_a?(Config)
        raise ArgumentError, 'pass either a Config or options, not both' unless options.empty?
        @config = config
      else
        @config = Config.new(config, **options)
      end
      @transport = Transport.new(@config)
    end

    attr_reader :config

    # Test hook: the wait between attempts, +lambda { |seconds, token| ... }+.
    def sleeper=(callable)
      @transport.sleeper = callable
    end

    # Closes the calling thread's connection. The client can be used again afterwards.
    def close
      @transport.close
    end

    # -- topics ---------------------------------------------------------------------------------------------

    # +POST /topics+. +request+ is a Taba::TopicRequest, or give its fields as options.
    def create_topic(request = nil, cancel: nil, **fields)
      request = request.nil? ? TopicRequest.new(fields) : TopicRequest.from(request)
      reply = send_call(Calls.create_topic(request), cancel)
      return get_topic(request.name, cancel: cancel) if reply.recovered
      Parser.topic(utf8(reply.body))
    end

    def get_topic(name, cancel: nil)
      Parser.topic(utf8(send_call(Calls.get_topic(name), cancel).body))
    end

    # +GET /topics+. Parameters left nil do not go in the query.
    def list_topics(limit: nil, cursor: nil, name: nil, cancel: nil)
      Parser.page(utf8(send_call(Calls.list_topics(limit, cursor, name), cancel).body))
    end

    # All topics, page by page. With a block, yields each topic.
    def each_topic(name: nil, cancel: nil)
      return to_enum(:each_topic, name: name, cancel: cancel) unless block_given?
      cursor = nil
      loop do
        page = list_topics(cursor: cursor, name: name, cancel: cancel)
        page.items.each { |topic| yield topic }
        cursor = page.next_cursor
        break if cursor.nil?
      end
    end

    def list_all_topics(name: nil, cancel: nil)
      each_topic(name: name, cancel: cancel).to_a
    end

    # +PATCH /topics/{topic}+: a real +PATCH+, with no verb-override header. +update+ is a Taba::TopicUpdate, or
    # give its fields as options.
    def update_topic(name, update = nil, cancel: nil, **fields)
      update = update.nil? ? TopicUpdate.new(fields) : TopicUpdate.from(update)
      Parser.topic(utf8(send_call(Calls.update_topic(name, update), cancel).body))
    end

    def delete_topic(name, cancel: nil)
      send_call(Calls.delete_topic(name), cancel)
      nil
    end

    # -- publishing -----------------------------------------------------------------------------------------

    # +POST /topics/{topic}/messages+. The body goes as bytes (a String is sent as it is), without validation. Set
    # +fifo: true+ when the topic is a FIFO queue: only there can a 502/504 be repeated. Options: content_type,
    # group_id, deduplication_id, request_id, traceparent, fifo.
    def publish(topic, body, cancel: nil, **options)
      reply = send_call(Calls.publish(topic, body, PublishOptions.new(options)), cancel)
      Parser.publish_response(utf8(reply.body))
    end

    # Serializes the value (with +JSON+) and publishes it as +application/json+. The options are a trailing hash, so
    # the value itself can be written as keywords: +publish_json('orders', order: 9001)+ (a Hash) or
    # +publish_json('orders', { order: 9001 }, group_id: 'g')+.
    def publish_json(topic, value, options = {})
      raise Error.client('client.invalid-request', 'value is required') if value.nil?
      begin
        text = ::JSON.generate(value, quirks_mode: true)
      rescue ::JSON::GeneratorError, EncodingError => e
        raise Error.client('client.invalid-request', "cannot serialize the value: #{e.message}")
      end
      publish(topic, text, **options.merge(content_type: 'application/json'))
    end

    # Publishes a JSON text the application already built, without serializing.
    def publish_raw_json(topic, text, **options)
      publish(topic, text, **options.merge(content_type: 'application/json'))
    end

    # Publishes a text (+Content-Type: text/plain; charset=utf-8+).
    def publish_text(topic, text, **options)
      publish(topic, text.to_s.dup.force_encoding(Encoding::UTF_8), **options.merge(content_type: 'text/plain; charset=utf-8'))
    end

    # +POST /topics/{topic}/messages/batch+: publishes many messages in one request (1 to the server's
    # +api.batch.max-messages+, 1000 by default). Each item is answered as its single publication and one that fails
    # does not undo the others (look at +ok?+). On a FIFO queue the items of a group are published in the order of
    # the array. A failure of the network is not repeated (it could duplicate).
    def publish_batch(topic, messages, cancel: nil)
      Parser.batch_results(utf8(send_call(Calls.publish_batch(topic, messages), cancel).body))
    end

    # -- consuming ------------------------------------------------------------------------------------------

    # +GET /topics/{topic}/messages+. The read time is +wait_time + 10+ seconds, not the +request_timeout+. A
    # timeout is not repeated (the node may have captured messages, which come back by themselves at the end of the
    # +visibility_timeout+). Returns an empty array if there are no messages. Options: max_messages,
    # visibility_timeout, wait_time, auto_create, volatile, idle_timeout, fifo, tags, request_id, traceparent.
    def consume(topic, cancel: nil, **options)
      fetch(topic, ConsumeOptions.new(options), cancel)
    end

    # +DELETE /topics/{topic}/messages/{id}?receiptToken=+. +message+ is what +consume+ returned, or its id.
    def ack(topic, message, receipt_token = nil, cancel: nil)
      id, token = reference(message, receipt_token)
      send_call(Calls.ack(topic, id, token), cancel)
      nil
    end

    # +PUT /topics/{topic}/messages/{id}/visibility?receiptToken=+.
    def change_visibility(topic, message, seconds, receipt_token = nil, cancel: nil)
      id, token = reference(message, receipt_token)
      reply = send_call(Calls.change_visibility(topic, id, token, seconds), cancel)
      Parser.visibility(utf8(reply.body))
    end

    # +POST /topics/{topic}/messages/ack+: acknowledges many messages +consume+ returned, in one request.
    def ack_batch(topic, messages, cancel: nil)
      Parser.batch_results(utf8(send_call(Calls.receipts(topic, 'ack', messages), cancel).body))
    end

    # +POST /topics/{topic}/messages/visibility+: the invisibility of each message, +seconds+ from now.
    def change_visibility_batch(topic, messages, seconds, cancel: nil)
      call = Calls.receipts(topic, 'visibility', messages, seconds)
      Parser.batch_results(utf8(send_call(call, cancel).body))
    end

    # Reads from the queue and hands each message to the block as a MessageContext. The message is deleted if the
    # block returns without raising (+context.delete = false+ keeps it). While the block runs, the invisibility is
    # renewed every third of it, by another thread. Defaults: one message at a time, 20 s of long polling, 120 s of
    # invisibility. If the block raises, the message is not deleted and the exception propagates. Returns how many
    # messages were handled.
    def consume_with(topic, cancel: nil, **options, &handler)
      handle_all(topic, options, cancel, false, handler)
    end

    # +consume_with+ with the body already parsed (with +JSON+) in +context.body+. A body that does not parse gives
    # +client.invalid-response+ before the block is called, and the message is not deleted.
    def consume_json(topic, cancel: nil, **options, &handler)
      handle_all(topic, options, cancel, true, handler)
    end

    # The consume loop: long polling (+wait_time+ 20 and one message at a time, if not set), yields each message to
    # the block and acknowledges by itself only if it returns without raising (a message that fails goes back to the
    # queue at the end of the +visibility_timeout+, and the exception goes to +on_error.call(message, error)+).
    # Blocks until +cancel+ is cancelled and then returns without an exception. A server error while consuming
    # propagates.
    def subscribe(topic, cancel: nil, on_error: nil, **options)
      raise ArgumentError, 'a block is required' unless block_given?
      cancel ||= CancelToken.new
      poll = Calls.subscribe_defaults(ConsumeOptions.new(options))
      until cancel.cancelled?
        begin
          messages = fetch(topic, poll, cancel)
        rescue Cancelled
          return nil
        end
        messages.each do |message|
          handled = false
          begin
            yield message
            handled = true
          rescue StandardError => e # the block is the application's code
            on_error.call(message, e) if on_error
          end
          settle(topic, message) if handled
          return nil if cancel.cancelled?
        end
      end
      nil
    end

    private

    def send_call(call, cancel)
      @transport.send_call(call, cancel)
    end

    def utf8(body)
      body.dup.force_encoding(Encoding::UTF_8)
    end

    def fetch(topic, options, cancel)
      Parser.messages(utf8(send_call(Calls.consume(topic, options), cancel).body))
    end

    def reference(message, receipt_token)
      return [message.id, message.receipt_token] if message.is_a?(Message)
      raise Error.client('client.invalid-request', 'receipt_token is required with a message id') if receipt_token.nil?
      [message.to_s, receipt_token]
    end

    def lost_or_gone?(error)
      [409, 404].include?(error.status)
    end

    # Without the token: the message was already handled, and acknowledging it must not be cut short.
    def settle(topic, message)
      ack(topic, message)
    rescue Error => e
      raise unless lost_or_gone?(e)
    end

    def handle_all(topic, options, cancel, parse, handler)
      raise ArgumentError, 'a block is required' if handler.nil?
      opts = Calls.handler_defaults(ConsumeOptions.new(options))
      handled = 0
      fetch(topic, opts, cancel).each do |message|
        body = nil
        if parse
          begin
            body = ::JSON.parse(message.body_text, quirks_mode: true)
          rescue ::JSON::ParserError => e
            raise Error.client('client.invalid-response', "body of message #{message.id} is not valid JSON: #{e.message}")
          end
        end
        context = MessageContext.new(message, opts.visibility_timeout,
                                     lambda { |seconds| change_visibility(topic, message, seconds, cancel: cancel) })
        context.body = body
        run_with_renewal(topic, context) { handler.call(context) }
        settle(topic, message) if context.delete
        handled += 1
      end
      handled
    end

    # Runs the handler while a second thread renews the invisibility.
    def run_with_renewal(topic, context)
      stop = CancelToken.new
      renewal = Thread.new { keep_alive(topic, context, stop) }
      renewal.abort_on_exception = false
      begin
        yield
      ensure
        stop.cancel
        renewal.join
      end
    end

    # The second thread: renews the invisibility until the handler finishes. Never raises.
    def keep_alive(topic, context, stop)
      until stop.cancelled?
        @transport.sleeper.call(Calls.renewal_interval(context.lease_seconds), stop)
        begin
          change_visibility(topic, context.message, context.lease_seconds, cancel: stop)
        rescue Error => e
          if lost_or_gone?(e)
            # The message already went back to the queue or was deleted: renewing is no longer of any use.
            context.lease_lost!
            return
          end
          # A transient failure (network, 5xx): tries again on the next round, before the deadline expires.
        end
      end
    rescue Cancelled
      nil
    ensure
      @transport.close # this thread's own connection
    end
  end
end
