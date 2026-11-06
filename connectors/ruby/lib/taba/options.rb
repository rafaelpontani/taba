require 'taba/error'

module Taba
  # Base of the request objects: +new(field: value, ...)+. An unknown field is a +client.invalid-request+ error, so a
  # typo does not go unnoticed.
  class Fields
    def self.fields(*names)
      @names = names
      attr_accessor(*names)
    end

    def self.names
      @names || []
    end

    def self.default(_name)
      nil
    end

    def initialize(values = {})
      self.class.names.each { |name| instance_variable_set("@#{name}", self.class.default(name)) }
      values.each do |key, value|
        unless self.class.names.include?(key.to_sym)
          raise Error.client('client.invalid-request', "unknown field of #{self.class.name}: #{key}")
        end
        send("#{key}=", value)
      end
    end

    # From an instance of the class or from a hash of fields (a copy: the caller's object is never changed).
    def self.from(value)
      return value.dup if value.is_a?(self)
      new(value || {})
    end
  end

  # +consume+ options. The connector sends only what was set: whatever is nil takes the server default.
  #
  # max_messages; visibility_timeout (seconds of invisibility); wait_time (seconds of long polling); auto_create;
  # volatile, idle_timeout, fifo and tags (only with auto_create); request_id; traceparent.
  class ConsumeOptions < Fields
    fields :max_messages, :visibility_timeout, :wait_time, :auto_create, :volatile, :idle_timeout, :fifo, :tags,
           :request_id, :traceparent

    def self.default(name)
      name == :auto_create ? false : nil
    end
  end

  # The options of +publish+: content_type (default application/json; +publish_text+ and +publish_json+ set their
  # own); group_id; deduplication_id; request_id; traceparent; fifo (true when the topic is a FIFO queue: only there
  # can a 502/504 be repeated).
  class PublishOptions < Fields
    fields :content_type, :group_id, :deduplication_id, :request_id, :traceparent, :fifo

    def self.default(name)
      name == :fifo ? false : nil
    end
  end

  # +POST /topics+. Only what was filled in is sent. +type+ is +queue+ or +distributor+; +prefixes+ (distributor
  # only) are the name prefixes of the queues each publication is copied to.
  class TopicRequest < Fields
    fields :name, :type, :tags, :prefixes, :volatile, :idle_timeout, :fifo, :max_message_bytes, :ttl, :max_deliveries
  end

  # +PATCH /topics/{topic}+: only what was given is sent. For +max_message_bytes+, +ttl+ and +max_deliveries+ a key
  # present with +nil+ sends +null+ (back to the server default); a key left out is not sent.
  #
  #   Taba::TopicUpdate.new(ttl: nil, max_deliveries: 5)
  class TopicUpdate
    KNOWN = [:tags, :prefixes, :idle_timeout, :max_message_bytes, :ttl, :max_deliveries].freeze
    NULLABLE = [:max_message_bytes, :ttl, :max_deliveries].freeze

    def initialize(values = {})
      @given = {}
      values.each do |key, value|
        unless KNOWN.include?(key.to_sym)
          raise Error.client('client.invalid-request', "unknown field of Taba::TopicUpdate: #{key}")
        end
        @given[key.to_sym] = value
      end
    end

    def self.from(value)
      return value if value.is_a?(TopicUpdate)
      new(value || {})
    end

    # The fields to send, name => value.
    attr_reader :given
  end

  # One message of +publish_batch+: the same fields as +publish+. The bytes travel in base64.
  class BatchMessage < Fields
    fields :body, :content_type, :group_id, :deduplication_id

    def initialize(body = '', values = {})
      super(values)
      @body = body
    end
  end
end
