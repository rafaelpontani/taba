require 'json'
require 'taba/error'
require 'taba/options'
require 'taba/transport'

module Taba
  # Builds the +ApiCall+ of each operation.
  module Calls
    # +consume+ waits +wait_time+ plus this slack (seconds), not the +request_timeout+.
    LONG_POLL_SLACK = 10.0

    HANDLER_MAX_MESSAGES = 1
    HANDLER_VISIBILITY_TIMEOUT = 120
    HANDLER_WAIT_TIME = 20

    module_function

    def encode(value)
      ::JSON.generate(value)
    rescue ::JSON::GeneratorError, EncodingError => e
      raise Error.client('client.invalid-request', "cannot serialize the request: #{e.message}")
    end

    def segment(value)
      Transport.segment(value)
    end

    def create_topic(request)
      body = { 'name' => request.name.to_s }.merge(topic_fields(request))
      call = ApiCall.new('POST', '/topics').with_json(encode(body))
      call.recover << [409, 'topic.already-exists']
      call
    end

    # +PUT /topics/{topic}+: the fields of +POST /topics+ without +name+ (it goes in the path).
    def put_topic(request, recreate)
      unless recreate.nil? || %w[if-empty always].include?(recreate.to_s)
        raise Error.client('client.invalid-request', "recreate must be 'if-empty' or 'always'")
      end
      call = ApiCall.new('PUT', "/topics/#{segment(request.name.to_s)}").with_json(encode(topic_fields(request)))
      call.with_query('recreate', recreate.to_s) unless recreate.nil?
      call
    end

    def topic_fields(request)
      body = { 'type' => request.type.to_s }
      body['tags'] = request.tags if request.tags && !request.tags.empty?
      body['prefixes'] = request.prefixes.to_a if request.prefixes && !request.prefixes.empty?
      [[:volatile, 'volatile'], [:idle_timeout, 'idleTimeout'], [:fifo, 'fifo'],
       [:max_message_bytes, 'maxMessageBytes'], [:ttl, 'ttl'], [:max_deliveries, 'maxDeliveries']].each do |attribute, name|
        value = request.send(attribute)
        body[name] = value unless value.nil?
      end
      body
    end

    def get_topic(name)
      ApiCall.new('GET', "/topics/#{segment(name)}")
    end

    def list_topics(limit, cursor, name)
      call = ApiCall.new('GET', '/topics')
      call.with_query('limit', limit) unless limit.nil?
      call.with_query('cursor', cursor) unless cursor.nil?
      call.with_query('name', name) unless name.nil?
      call
    end

    def update_topic(name, update)
      body = {}
      update.given.each do |key, value|
        wire = key.to_s.gsub(/_([a-z])/) { Regexp.last_match(1).upcase }
        if TopicUpdate::NULLABLE.include?(key)
          body[wire] = value # nil goes as JSON null: back to the server default
        elsif !value.nil?
          body[wire] = key == :prefixes ? value.to_a : value
        end
      end
      ApiCall.new('PATCH', "/topics/#{segment(name)}").with_json(encode(body))
    end

    def delete_topic(name, if_empty = false)
      call = ApiCall.new('DELETE', "/topics/#{segment(name)}")
      call.with_query('ifEmpty', 'true') if if_empty
      call.recover << [404, 'topic.not-found']
      call
    end

    def bytes(body)
      body.nil? ? '' : body.to_s
    end

    def publish(topic, body, options)
      call = ApiCall.new('POST', "/topics/#{segment(topic)}/messages")
      call.body = bytes(body)
      call.content_type = options.content_type || 'application/json'
      call.request_id = options.request_id
      call.traceparent = options.traceparent
      call.retry_gateway = options.fifo ? true : false
      call.with_query('groupId', options.group_id) unless options.group_id.nil?
      call.with_query('deduplicationId', options.deduplication_id) unless options.deduplication_id.nil?
      call
    end

    def publish_batch(topic, messages)
      items = messages.map do |m|
        unless m.is_a?(BatchMessage)
          raise Error.client('client.invalid-request', 'publish_batch expects Taba::BatchMessage objects')
        end
        item = { 'body' => [bytes(m.body)].pack('m0'), 'bodyEncoding' => 'base64' }
        item['contentType'] = m.content_type unless m.content_type.nil?
        item['groupId'] = m.group_id unless m.group_id.nil?
        item['deduplicationId'] = m.deduplication_id unless m.deduplication_id.nil?
        item
      end
      ApiCall.new('POST', "/topics/#{segment(topic)}/messages/batch").with_json(encode('messages' => items))
    end

    # The ack (+route+ +ack+) or the change of invisibility (+visibility+) of many messages in one request.
    def receipts(topic, route, messages, seconds = nil)
      items = messages.map do |m|
        item = { 'id' => m.id, 'receiptToken' => m.receipt_token }
        item['visibilityTimeout'] = seconds.to_i unless seconds.nil?
        item
      end
      ApiCall.new('POST', "/topics/#{segment(topic)}/messages/#{route}").with_json(encode('messages' => items))
    end

    def consume(topic, o)
      call = ApiCall.new('GET', "/topics/#{segment(topic)}/messages")
      call.with_query('maxMessages', o.max_messages) unless o.max_messages.nil?
      call.with_query('visibilityTimeout', o.visibility_timeout) unless o.visibility_timeout.nil?
      call.with_query('waitTime', o.wait_time) unless o.wait_time.nil?
      if o.auto_create
        call.with_query('autoCreate', 'true')
        call.with_query('volatile', o.volatile) unless o.volatile.nil?
        call.with_query('idleTimeout', o.idle_timeout) unless o.idle_timeout.nil?
        call.with_query('fifo', o.fifo) unless o.fifo.nil?
        (o.tags || {}).sort_by { |k, _| k.to_s }.each { |k, v| call.with_query("tag.#{k}", v) }
      end
      call.request_id = o.request_id
      call.traceparent = o.traceparent
      call.timeout = (o.wait_time || 0).to_f + LONG_POLL_SLACK
      call
    end

    def ack(topic, id, receipt_token)
      ApiCall.new('DELETE', "/topics/#{segment(topic)}/messages/#{segment(id)}").with_query('receiptToken', receipt_token)
    end

    def change_visibility(topic, id, receipt_token, seconds)
      call = ApiCall.new('PUT', "/topics/#{segment(topic)}/messages/#{segment(id)}/visibility")
      call.with_query('receiptToken', receipt_token)
      call.with_json(encode('visibilityTimeout' => seconds.to_i))
    end

    # One message at a time, 20 s of long polling and 120 s of invisibility, which the renewal keeps.
    def handler_defaults(o)
      o.max_messages = HANDLER_MAX_MESSAGES if o.max_messages.nil?
      o.visibility_timeout = HANDLER_VISIBILITY_TIMEOUT if o.visibility_timeout.nil?
      o.wait_time = HANDLER_WAIT_TIME if o.wait_time.nil?
      o
    end

    def subscribe_defaults(o)
      o.wait_time = HANDLER_WAIT_TIME if o.wait_time.nil?
      o.max_messages = HANDLER_MAX_MESSAGES if o.max_messages.nil?
      o
    end

    # A third of the invisibility (and never all of it), so a failed renewal can be tried again in time.
    def renewal_interval(lease_seconds)
      [1, lease_seconds].max / 3.0
    end
  end
end
