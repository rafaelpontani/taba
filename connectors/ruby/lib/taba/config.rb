require 'taba/version'
require 'taba/error'

module Taba
  # The client configuration (doc 21). It does not read environment variables: +TABA_*+ is the prefix of the server
  # configuration; whoever uses the connector reads their own and fills this object.
  #
  #   Taba::Config.new('http://localhost:8080', token: token)
  class Config
    FIELDS = [:base_url, :token, :connect_timeout, :request_timeout, :max_attempts, :user_agent, :ca_file, :ca_path].freeze

    # Address of the client API load balancer (+http://host:port+ or +https://...+). Required.
    attr_accessor :base_url
    # Goes in +Authorization: Bearer+. Without it, the call goes without the header.
    attr_accessor :token
    # Seconds to open the connection.
    attr_accessor :connect_timeout
    # Seconds of an ordinary call (each wait for the response). +consume+ uses +wait_time + 10+.
    attr_accessor :request_timeout
    # Attempts, counting the first.
    attr_accessor :max_attempts
    # Goes in +User-Agent+. Add the application name.
    attr_accessor :user_agent
    # An own CA bundle file (a private CA, for example). Default: the system's. The certificate is always verified.
    attr_accessor :ca_file
    # An own directory of CA certificates.
    attr_accessor :ca_path

    def initialize(base_url = nil, **options)
      @base_url = base_url
      @token = nil
      @connect_timeout = 5.0
      @request_timeout = 30.0
      @max_attempts = 3
      @user_agent = "taba-ruby/#{VERSION}"
      @ca_file = nil
      @ca_path = nil
      options.each do |key, value|
        unless FIELDS.include?(key)
          raise Error.client('client.invalid-config', "unknown configuration field: #{key}")
        end
        send("#{key}=", value)
      end
    end

    # A Config from itself or an address (the options only apply to an address).
    def self.from(value, **options)
      return value if value.is_a?(Config)
      new(value, **options)
    end
  end
end
