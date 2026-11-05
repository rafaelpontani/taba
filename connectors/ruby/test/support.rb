$LOAD_PATH.unshift(File.expand_path('../../lib', __FILE__))

require 'json'
require 'socket'
require 'thread'
require 'minitest/autorun'
require 'taba'

BaseTest = defined?(Minitest::Test) ? Minitest::Test : MiniTest::Unit::TestCase

# A local HTTP server that records every request and answers from a script (the ScriptedHandler of the C# connector).
class ScriptedServer
  Recorded = Struct.new(:method, :path, :raw_query, :headers, :body) do
    def query
      raw_query.to_s.split('&').each_with_object({}) do |pair, out|
        key, value = pair.split('=', 2)
        out[URI.decode_www_form_component(key)] = URI.decode_www_form_component(value.to_s)
      end
    end

    def text
      body.dup.force_encoding(Encoding::UTF_8)
    end
  end

  DESTROY = :destroy
  HANG = :hang

  attr_reader :requests

  def initialize(port = 0)
    @requests = []
    @script = []
    @fallback = ScriptedServer.reply(200, '{}')
    @mutex = Mutex.new
    @stop = false
    @sockets = []
    @server = TCPServer.new('127.0.0.1', port)
    @thread = Thread.new { accept_loop }
  end

  def self.reply(status = 200, body = '', headers = {})
    body = JSON.generate(body) unless body.is_a?(String)
    { status: status, body: body, headers: headers }
  end

  def port
    @server.addr[1]
  end

  def url
    "http://127.0.0.1:#{port}"
  end

  # Replies are hashes from +reply+, lambdas of the request returning one, DESTROY or HANG.
  def reply(*answers)
    @mutex.synchronize { @script.concat(answers) }
    self
  end

  def fallback(answer = nil, &block)
    @mutex.synchronize { @fallback = block || answer }
    self
  end

  # Answers the consume calls (GET) with these message batches, in order, then with an empty one; the rest gets a
  # plain 200.
  def batches(*batches)
    queue = batches.dup
    fallback do |request|
      if request.method != 'GET'
        ScriptedServer.reply(200, 'id' => 'x', 'invisibleUntil' => 'x')
      else
        batch = @mutex.synchronize { queue.shift } || []
        ScriptedServer.reply(200, 'messages' => batch)
      end
    end
  end

  def count
    @mutex.synchronize { @requests.size }
  end

  def all
    @mutex.synchronize { @requests.dup }
  end

  def close
    @stop = true
    @server.close rescue nil
    @mutex.synchronize { @sockets.dup }.each { |s| s.close rescue nil }
  end

  private

  def accept_loop
    until @stop
      client = @server.accept
      @mutex.synchronize { @sockets << client }
      Thread.new(client) { |socket| serve(socket) }
    end
  rescue IOError, SystemCallError
    nil
  end

  def serve(socket)
    loop do
      line = socket.gets
      break if line.nil?
      method, target = line.split(' ')
      headers = {}
      while (header = socket.gets) && header != "\r\n"
        name, value = header.split(':', 2)
        headers[name.strip.downcase] = value.strip
      end
      length = headers['content-length'].to_i
      body = length > 0 ? socket.read(length) : ''
      path, query = target.split('?', 2)
      request = Recorded.new(method, path, query, headers, body.force_encoding(Encoding::BINARY))
      answer = @mutex.synchronize do
        @requests << request
        @script.empty? ? @fallback : @script.shift
      end
      answer = answer.call(request) if answer.respond_to?(:call)
      if answer == DESTROY
        socket.close
        break
      end
      if answer == HANG
        sleep 0.05 until @stop
        break
      end
      data = answer[:body].dup.force_encoding(Encoding::BINARY)
      head = "HTTP/1.1 #{answer[:status]} X\r\nContent-Type: application/json\r\nContent-Length: #{data.bytesize}\r\n"
      answer[:headers].each { |k, v| head << "#{k}: #{v}\r\n" }
      socket.write(head + "\r\n" + data)
    end
  rescue IOError, SystemCallError
    nil
  ensure
    socket.close rescue nil
  end
end

# A sleep that does not wait and records what was asked.
class FakeSleep
  attr_reader :waits

  def initialize
    @waits = []
    @mutex = Mutex.new
  end

  def call(seconds, token)
    @mutex.synchronize { @waits << seconds }
    raise Taba::Cancelled if token.cancelled?
  end
end

# A sleep that never ends by itself: the renewal does not spin in tests that do not look at it.
BLOCKING_SLEEP = lambda do |_seconds, token|
  raise Taba::Cancelled if token.wait(60)
end

class ClientCase < BaseTest
  def setup
    @server = ScriptedServer.new
    @sleep = FakeSleep.new
    @client = Taba::Client.new(@server.url)
    @client.sleeper = @sleep
  end

  def teardown
    @client.close
    @server.close
  end

  def reply(*args)
    ScriptedServer.reply(*args)
  end

  def assert_error(code, &block)
    error = assert_raises(Taba::Error, &block)
    assert_equal code, error.code, error.message
    error
  end
end

def topic_json(name, extra = {})
  { 'id' => "t-#{name}", 'name' => name, 'type' => 'queue', 'createdAt' => '2026-10-27T10:00:00Z' }.merge(extra)
end

def message_json(id, body, extra = {})
  { 'id' => id, 'body' => body, 'contentType' => 'application/json', 'receiptToken' => "rt-#{id}", 'deliveryCount' => 1,
    'publishedAt' => '2026-10-27T10:00:00Z', 'invisibleUntil' => '2026-10-27T10:02:00Z' }.merge(extra)
end

def wait_for(timeout = 5)
  deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + timeout
  until yield
    raise 'condition not met' if Process.clock_gettime(Process::CLOCK_MONOTONIC) > deadline
    sleep 0.005
  end
end
