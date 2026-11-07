require 'taba/error'
require 'taba/json_fields'

module Taba
  # The models of the client API and the reading of its JSON responses.

  # What +create_or_update_topic+ did and the topic as it is now.
  class TopicPut
    # It did not exist (+201+).
    CREATED = 'created'.freeze
    # It existed: the mutable fields are now the declared ones, or already were (+200+).
    UPDATED = 'updated'.freeze
    # It was removed and created again with a new +id+ (+201+ with +Taba-Topic-Recreated: true+).
    RECREATED = 'recreated'.freeze

    # +topic+ (a Taba::Topic) and +outcome+ (+created+, +updated+ or +recreated+).
    attr_reader :topic, :outcome

    def initialize(topic, outcome)
      @topic, @outcome = topic, outcome
    end
  end

  class Topic
    attr_reader :id, :name, :type, :created_at, :fifo, :volatile, :raw
    # id, name, type (+queue+ or +distributor+), created_at (RFC 3339 text), fifo, volatile, and raw (the topic
    # JSON, as the server returned it).
    def initialize(id, name, type, created_at, fifo, volatile, raw)
      @id, @name, @type, @created_at, @fifo, @volatile, @raw = id, name, type, created_at, fifo, volatile, raw
    end
  end

  class Page
    attr_reader :items, :next_cursor, :raw
    # The topics of the page; next_cursor is the cursor of the next page, or nil on the last one.
    def initialize(items, next_cursor, raw)
      @items, @next_cursor, @raw = items, next_cursor, raw
    end
  end

  # A delivered message. The +body+ always comes out as bytes (doc 21, "The delivered message"): for JSON, the JSON
  # text as it came; for +text/*+, the UTF-8 text; for base64, the bytes already decoded (a binary String).
  class Message
    attr_reader :id, :body, :content_type, :receipt_token, :delivery_count, :published_at, :invisible_until,
                :group_id, :deduplication_id, :dead_lettered_at, :dead_letter_reason, :raw

    def initialize(attributes)
      attributes.each { |name, value| instance_variable_set("@#{name}", value) }
    end

    # The +body+ as UTF-8 text.
    def body_text
      @body.dup.force_encoding(Encoding::UTF_8)
    end

    # The +body+ read as JSON.
    def json
      ::JSON.parse(body_text, quirks_mode: true)
    rescue ::JSON::ParserError => e
      raise Error.client('client.invalid-response', "body of message #{@id} is not valid JSON: #{e.message}")
    end
  end

  # The copy of a publication made by a distributor to a queue.
  class DistributorDelivery
    attr_reader :topic, :id, :error_code, :error_message

    def initialize(topic, id, error_code, error_message)
      @topic, @id, @error_code, @error_message = topic, id, error_code, error_message
    end

    def succeeded?
      @error_code.nil?
    end
  end

  class PublishResponse
    # id (queue; nil in a distributor's response); duplicate (FIFO queue only: true when it repeated a publication
    # within the deduplication window); matched (distributor only: how many queues matched); deliveries.
    attr_reader :id, :duplicate, :matched, :deliveries, :raw

    def initialize(id, duplicate, matched, deliveries, raw)
      @id, @duplicate, @matched, @deliveries, @raw = id, duplicate, matched, deliveries, raw
    end
  end

  class VisibilityResponse
    attr_reader :id, :invisible_until, :raw

    def initialize(id, invisible_until, raw)
      @id, @invisible_until, @raw = id, invisible_until, raw
    end
  end

  # The result of one item of a batch, in the order of the request: the +status+ the single call would have
  # answered and what it would have returned. An item that failed does not raise: +ok?+ is false.
  class BatchResult
    # status: 201 published, 200 a FIFO duplicate, a distributor or a new invisibility, 204 acked, or the error's.
    attr_reader :status, :id, :duplicate, :matched, :invisible_until, :error_code, :error_message, :raw

    def initialize(status, id, duplicate, matched, invisible_until, error_code, error_message, raw)
      @status, @id, @duplicate, @matched, @invisible_until = status, id, duplicate, matched, invisible_until
      @error_code, @error_message, @raw = error_code, error_message, raw
    end

    def ok?
      @error_code.nil? && @status < 300
    end
  end

  # Reading the responses.
  module Parser
    module_function

    def invalid(what, error)
      Error.client('client.invalid-response', "#{what}: #{error.message}")
    end

    def fields(raw, what)
      JsonFields.parse(raw)
    rescue JsonError => e
      raise invalid(what, e)
    end

    def json_type?(content_type)
      media = content_type.split(';').first.to_s.strip.downcase
      media == 'application/json' || media.end_with?('+json')
    end

    def topic(raw)
      f = fields(raw, 'topic')
      Topic.new(f.string('id') || '', f.string('name') || '', f.string('type') || '', f.string('createdAt') || '',
                f.boolean('fifo'), f.boolean('volatile'), raw)
    rescue JsonError => e
      raise invalid('topic', e)
    end

    def message(raw)
      f = fields(raw, 'message')
      content_type = f.string('contentType') || 'application/json'
      body = f.raw('body')
      if body.nil?
        data = ''.force_encoding(Encoding::BINARY)
      elsif f.string('bodyEncoding') == 'base64'
        text = JsonText.decode_string(body)
        begin
          data = text.unpack('m0').first
        rescue ArgumentError
          raise Error.client('client.invalid-response', 'invalid base64 body')
        end
      elsif json_type?(content_type)
        data = body.dup
      else
        data = JsonText.decode_string(body)
      end
      Message.new(
        id: f.string('id') || '', body: data, content_type: content_type, receipt_token: f.string('receiptToken') || '',
        delivery_count: f.integer('deliveryCount') || 0, published_at: f.string('publishedAt') || '',
        invisible_until: f.string('invisibleUntil') || '', group_id: f.string('groupId'),
        deduplication_id: f.string('deduplicationId'), dead_lettered_at: f.string('deadLetteredAt'),
        dead_letter_reason: f.string('deadLetterReason'), raw: raw
      )
    rescue JsonError => e
      raise invalid('message', e)
    end

    def messages(raw)
      f = fields(raw, 'consume')
      listing = f.raw('messages')
      (listing.nil? ? [] : JsonText.items(listing)).map { |item| message(item) }
    rescue JsonError => e
      raise invalid('messages', e)
    end

    def page(raw)
      f = fields(raw, 'page')
      listing = f.raw('items')
      items = (listing.nil? ? [] : JsonText.items(listing)).map { |item| topic(item) }
      Page.new(items, f.string('nextCursor'), raw)
    rescue JsonError => e
      raise invalid('page items', e)
    end

    # The +error+ object of a delivery or a batch item: [code, message], or nil when there is none.
    def error_of(f)
      error = f.raw('error')
      return nil if error.nil? || !error.start_with?('{')
      e = JsonFields.parse(error)
      [e.string('code') || '', e.string('message')]
    end

    def publish_response(raw)
      f = fields(raw, 'publication')
      listing = f.raw('deliveries')
      deliveries = (listing.nil? ? [] : JsonText.items(listing)).map do |item|
        d = JsonFields.parse(item)
        error = error_of(d)
        DistributorDelivery.new(d.string('topic') || '', d.string('id'), error && error[0], error && error[1])
      end
      PublishResponse.new(f.string('id'), f.boolean('duplicate'), f.integer('matched'), deliveries, raw)
    rescue JsonError => e
      raise invalid('publication', e)
    end

    # The +results+ of a batch, one per item in the order of the request.
    def batch_results(raw)
      f = fields(raw, 'batch')
      listing = f.raw('results')
      (listing.nil? ? [] : JsonText.items(listing)).map do |item|
        r = JsonFields.parse(item)
        error = error_of(r)
        BatchResult.new(r.integer('status') || 0, r.string('id'), r.boolean('duplicate'), r.integer('matched'),
                        r.string('invisibleUntil'), error && error[0], error && error[1], item)
      end
    rescue JsonError => e
      raise invalid('batch results', e)
    end

    def visibility(raw)
      f = fields(raw, 'visibility')
      VisibilityResponse.new(f.string('id') || '', f.string('invisibleUntil') || '', raw)
    rescue JsonError => e
      raise invalid('visibility', e)
    end
  end
end
