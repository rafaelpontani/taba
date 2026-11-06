require 'net/http'
require 'openssl'
require 'uri'
require 'taba/error'
require 'taba/cancel_token'
require 'taba/config'
require 'taba/json_fields'

module Taba
  # The description of a request to the API: method, path, query, body, headers and the rules of the call (timeout,
  # retry of 502/504, responses that count as success after a 503).
  class ApiCall
    attr_accessor :method, :path, :query, :body, :content_type, :request_id, :traceparent
    # Overrides the total time of the call in seconds (+consume+ uses +wait_time + 10+).
    attr_accessor :timeout
    # Retries 502 and 504: only publishing to a FIFO queue, where the primary deduplicates.
    attr_accessor :retry_gateway
    # After a 503, these [status, code] count as success (the operation had already been applied).
    attr_accessor :recover

    # +path+ has the segments already encoded (+/topics/orders+).
    def initialize(method, path)
      @method = method
      @path = path
      @query = []
      @body = nil
      @recover = []
      @retry_gateway = false
    end

    def with_query(key, value)
      @query << [key, value.to_s]
      self
    end

    def with_json(text)
      @body = text
      self
    end
  end

  ApiReply = Struct.new(:status, :body, :recovered)

  # The client's HTTP transport: builds the request, sends it, reads the body, turns the API error into a
  # Taba::Error and does the call retries of the common contract (doc 21).
  class Transport
    # Net::HTTP repeats an idempotent request (GET, PUT, DELETE) once by itself after a timeout or a reset. The
    # contract says when to repeat (a timeout in +consume+ is never safe to repeat), so the requests report a method
    # Net::HTTP does not know as idempotent; the line sent on the wire is still the real verb.
    module NoAutomaticRetry
      def method
        "TABA-#{@method}"
      end
    end

    REQUESTS = {
      'GET' => Net::HTTP::Get, 'POST' => Net::HTTP::Post, 'PUT' => Net::HTTP::Put,
      'DELETE' => Net::HTTP::Delete, 'PATCH' => Net::HTTP::Patch
    }.each_with_object({}) do |(verb, base), out|
      out[verb] = Class.new(base) { include NoAutomaticRetry }
    end.freeze

    CONNECT_ERRORS = [Errno::ECONNREFUSED, SocketError, Errno::ENETUNREACH, Errno::EHOSTUNREACH].freeze
    STALE_ERRORS = [EOFError, Errno::ECONNRESET, Errno::EPIPE].freeze

    # A failure before the body was sent (refused, DNS): the call can be repeated like a 503.
    class Retry < StandardError
      attr_reader :error

      def initialize(error)
        super(error.message)
        @error = error
      end
    end

    attr_reader :config
    # The wait between attempts: +sleep(seconds, token)+ raises Cancelled. Tests replace it.
    attr_accessor :sleeper

    def self.segment(value)
      URI.encode_www_form_component(value.to_s).gsub('+', '%20')
    end

    def initialize(config)
      @config = config
      @scheme, @host, @port, @prefix = validate(config.base_url)
      @key = "taba-connection-#{object_id}"
      @sleeper = lambda do |seconds, token|
        raise Cancelled if token.wait(seconds)
      end
    end

    # Closes the calling thread's connection.
    def close
      drop
    end

    # Makes the call, retrying only what the contract says to retry: 429 and 503 (waiting for the +Retry-After+, or
    # 1 s, 2 s, 4 s), 502 and 504 only with +retry_gateway+, and a failure to connect (refused, DNS). Never a 4xx
    # (except 429), nor a failure after the body was sent, nor a timeout.
    def send_call(call, cancel = nil)
      cancellable = !cancel.nil?
      cancel ||= CancelToken.new
      timeout = call.timeout || @config.request_timeout
      maximum = [1, @config.max_attempts].max
      after_retryable = false
      attempt = 0
      loop do
        attempt += 1
        backoff = (1 << [attempt - 1, 6].min).to_f
        begin
          status, headers, body = once(call, timeout, cancel, cancellable)
          return ApiReply.new(status, body, false) if status < 400
          if after_retryable && call.recover.any? { |s, c| s == status && c == error_code_of(body) }
            return ApiReply.new(status, body, true)
          end
          error = error_from(status, headers, body)
          retry_it = status == 429 || status == 503 || ([502, 504].include?(status) && call.retry_gateway)
          raise error if !retry_it || attempt >= maximum
          after_retryable ||= status == 503
          wait = error.retry_after.nil? ? backoff : error.retry_after.to_f
        rescue Retry => r
          raise r.error if attempt >= maximum
          after_retryable = true
          wait = backoff
        end
        @sleeper.call(wait, cancel)
      end
    end

    private

    def validate(base_url)
      uri = nil
      if base_url.is_a?(String) && !base_url.strip.empty?
        begin
          uri = URI.parse(base_url.strip)
        rescue URI::InvalidURIError
          uri = nil
        end
      end
      unless uri && %w(http https).include?(uri.scheme) && uri.host && !uri.host.empty?
        shown = base_url.is_a?(String) && !base_url.strip.empty? ? base_url : 'empty'
        raise Error.client('client.invalid-config', "base_url must be an http:// or https:// address (received: #{shown})")
      end
      [uri.scheme, uri.host.sub(/\A\[(.*)\]\z/, '\1'), uri.port, uri.path.sub(%r{/+\z}, '')]
    end

    def new_connection
      http = Net::HTTP.new(@host, @port)
      http.open_timeout = @config.connect_timeout
      http.keep_alive_timeout = 30 if http.respond_to?(:keep_alive_timeout=)
      http.max_retries = 0 if http.respond_to?(:max_retries=)
      if @scheme == 'https'
        http.use_ssl = true
        http.verify_mode = OpenSSL::SSL::VERIFY_PEER
        http.ca_file = @config.ca_file if @config.ca_file
        http.ca_path = @config.ca_path if @config.ca_path
        if http.respond_to?(:min_version=)
          http.min_version = :TLS1_2
        else
          http.ssl_version = :TLSv1_2
        end
      end
      http
    end

    def connection
      Thread.current.thread_variable_get(@key)
    end

    def drop
      http = connection
      Thread.current.thread_variable_set(@key, nil)
      return if http.nil?
      begin
        http.finish if http.started?
      rescue StandardError, IOError
        nil
      end
    end

    # The socket of the connection, to close it from another thread when the call is cancelled.
    def abort_connection(http)
      socket = http.instance_variable_get(:@socket)
      socket.io.close if socket
    rescue StandardError, IOError
      nil
    end

    def url_path(call)
      path = @prefix + call.path
      unless call.query.empty?
        path += '?' + call.query.map { |k, v| "#{Transport.segment(k)}=#{Transport.segment(v)}" }.join('&')
      end
      path
    end

    def build_request(call)
      request = REQUESTS.fetch(call.method).new(url_path(call))
      request['Accept'] = 'application/json'
      request['Accept-Encoding'] = 'identity'
      request['User-Agent'] = @config.user_agent
      request['Authorization'] = "Bearer #{@config.token}" if @config.token && !@config.token.empty?
      request['X-Request-Id'] = call.request_id if call.request_id
      request['traceparent'] = call.traceparent if call.traceparent
      unless call.body.nil?
        request['Content-Type'] = call.content_type || 'application/json'
        request.body = call.body
      end
      request
    end

    def now
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end

    def describe(error)
      error.message.to_s.empty? ? error.class.name : "#{error.class.name}: #{error.message}"
    end

    # The exchange itself: connects if needed, sends the request and reads the whole response. +state+ tells the
    # caller whether the request had started to go out when it failed.
    def exchange(http, call, deadline, state)
      unless http.started?
        begin
          http.start
        rescue Net::OpenTimeout
          raise Error.client('client.network', "timed out connecting to #{@host}")
        end
      end
      remaining = deadline - now
      raise Net::ReadTimeout if remaining <= 0
      http.read_timeout = remaining
      http.write_timeout = remaining if http.respond_to?(:write_timeout=)
      state[:sent] = true
      response = http.request(build_request(call))
      headers = {}
      response.each_header { |name, value| headers[name.downcase] = value }
      [response.code.to_i, headers, response.body.to_s.dup.force_encoding(Encoding::BINARY)]
    end

    # Runs the exchange in a helper thread, so a cancellation returns at once: closing the socket of the call in
    # flight does not wake a thread blocked in a read on every Ruby version. The helper ends by itself when the
    # socket closes or the read times out.
    def exchange_cancellable(cancel, http, call, deadline, state)
      outcome = Queue.new
      worker = Thread.new do
        begin
          outcome << [:ok, exchange(http, call, deadline, state)]
        rescue Exception => e # rubocop:disable Lint/RescueException - handed to the caller, which raises it
          outcome << [:error, e]
        end
      end
      worker.abort_on_exception = false
      wake = lambda { outcome << [:cancelled] }
      cancel.attach(wake)
      kind, value = outcome.pop
      cancel.detach(wake)
      case kind
      when :ok then value
      when :error then raise value
      else
        abort_connection(http)
        raise Cancelled
      end
    end

    # One attempt. A stale pooled connection (the server closed it while idle) is tried again once on a new one.
    def once(call, timeout, cancel, cancellable)
      deadline = now + timeout
      [true, false].each do |stale_retry|
        cancel.raise_if_cancelled
        http = connection
        reused = !http.nil? && http.started?
        if http.nil?
          http = new_connection
          Thread.current.thread_variable_set(@key, http)
        end
        state = { sent: false }
        begin
          result = if cancellable
                     exchange_cancellable(cancel, http, call, deadline, state)
                   else
                     exchange(http, call, deadline, state)
                   end
          drop if result[1]['connection'].to_s.downcase == 'close'
          return result
        rescue Cancelled
          Thread.current.thread_variable_set(@key, nil) # the helper may still hold this connection
          raise
        rescue Error
          drop
          raise
        rescue StandardError, IOError => e
          drop
          raise Cancelled if cancel.cancelled?
          raise Error.client('client.timeout', 'timed out waiting for the response') if e.is_a?(Timeout::Error)
          error = Error.client('client.network', describe(e))
          raise Retry.new(error) if !state[:sent] && !reused && CONNECT_ERRORS.any? { |k| e.is_a?(k) }
          next if reused && stale_retry && STALE_ERRORS.any? { |k| e.is_a?(k) }
          raise error
        end
      end
      raise 'unreachable'
    end

    def error_code_of(body)
      JsonFields.parse(utf8(body)).string('code')
    rescue JsonError
      nil
    end

    def utf8(body)
      body.dup.force_encoding(Encoding::UTF_8)
    end

    def error_from(status, headers, body)
      text = utf8(body)
      code = message = request_id = details = nil
      begin
        fields = JsonFields.parse(text)
        code = fields.string('code')
        unless code.nil?
          message = fields.string('message') || ''
          request_id = fields.string('requestId')
          details = fields.raw('details')
        end
      rescue JsonError
        nil # Not an ErrorResponse: falls to the generic code below.
      end
      if code.nil?
        code = "http.#{status}"
        trimmed = text.strip
        message = trimmed.empty? ? "response #{status} with no body" : trimmed[0, 300]
      end
      header = headers['retry-after']
      retry_after = header && header.strip =~ /\A[0-9]+\z/ ? header.strip.to_i : nil
      Error.new(status, code, message, request_id: request_id || headers['x-request-id'], retry_after: retry_after,
                                       details: details, body: body)
    end
  end
end
