module Taba
  # The only kind of error of the connector (doc 21, "The common contract"): the API +ErrorResponse+, or a failure of
  # the client itself (+status+ 0 and a +code+ +client.network+, +client.timeout+, +client.invalid-config+,
  # +client.invalid-response+ or +client.invalid-request+). Test the +code+, never the +message+.
  class Error < StandardError
    # The HTTP status, or zero on a failure of the client itself.
    attr_reader :status
    # The +group.reason+ code of the error.
    attr_reader :code
    # The +requestId+ of the +ErrorResponse+, or the +X-Request-Id+ header.
    attr_reader :request_id
    # The seconds of the +Retry-After+, if the response carried it (nil otherwise).
    attr_reader :retry_after
    # The +details+ of the +ErrorResponse+, as JSON text.
    attr_reader :details
    # The response body, as it came (empty on a client failure).
    attr_reader :body

    # The message of the +ErrorResponse+ alone (+message+ and +to_s+ also carry the code and the request id).
    attr_reader :reason

    def initialize(status, code, message, request_id: nil, retry_after: nil, details: nil, body: '')
      super(message)
      @reason = message
      @status = status
      @code = code || ''
      @request_id = request_id
      @retry_after = retry_after
      @details = details
      @body = body
    end

    def self.client(code, message)
      new(0, code, message)
    end

    # A failure of the client itself, not a server response.
    def client_side?
      @status == 0
    end

    def to_s
      suffix = @request_id.nil? ? '' : " (requestId #{@request_id})"
      "#{@code}: #{@reason}#{suffix}"
    end
  end

  # The call was cancelled through a CancelToken. It is not a Taba::Error: it is not a failure of the call.
  class Cancelled < StandardError
    def initialize(message = 'the call was cancelled')
      super
    end
  end
end
