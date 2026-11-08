# The Ruby driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and runs
# each line against a real Taba through the connector, in the style of the language.
#
#   ruby contract/driver.rb --url http://host:8080 --token <token> --prefix ct-ruby-0101 --lang ruby \
#       --scenario ../contract/scenarios/core.scn

$LOAD_PATH.unshift(File.expand_path('../../lib', __FILE__))
require 'taba'

ARGS = { scenario: [] }
ARGV.each_slice(2) do |key, value|
  name = key.sub(/\A--/, '').to_sym
  name == :scenario ? ARGS[:scenario] << value : ARGS[name] = value
end

# ---- the scenario language ------------------------------------------------------------------------------

def unquote(text)
  text.gsub(/(?:%[0-9A-Fa-f]{2})+/) { |run| [run.delete('%')].pack('H*').force_encoding(Encoding::UTF_8) }
end

# Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched.
def expand(value, variables)
  out = ''
  last = 0
  value.scan(/\{p\}|\{lang\}|\$([A-Za-z0-9_]+)\.([A-Za-z0-9_]+)/) do
    match = Regexp.last_match
    out << unquote(value[last...match.begin(0)])
    case match[0]
    when '{p}' then out << ARGS[:prefix]
    when '{lang}' then out << ARGS[:lang]
    else out << ((variables[match[1]] || {})[match[2]] || '')
    end
    last = match.end(0)
  end
  out << unquote(value[last..-1])
end

def parse_line(text, variables)
  head, tail = text.split(' => ', 2)
  parts = head.split
  params = {}
  parts[1..-1].each do |pair|
    key, value = pair.split('=', 2)
    params[key] = expand(value || '', variables)
  end
  expects = tail.to_s.split.map do |token|
    index = token =~ /[=~]/
    [token[0...index], token[index], expand(token[(index + 1)..-1], variables)]
  end
  [parts[0], params, expects]
end

# ---- helpers --------------------------------------------------------------------------------------------

class SkipLine < StandardError; end

def num(p, key)
  p.key?(key) ? p[key].to_i : nil
end

def flag(p, key)
  p.key?(key) ? p[key] == 'true' : nil
end

def items(p, key)
  p.key?(key) ? p[key].split(',').reject(&:empty?) : nil
end

def tags_of(p, key)
  p.key?(key) ? Hash[p[key].split(',').map { |t| t.split(':', 2) }] : nil
end

# Only what the scenario gave goes into the fields: absent leaves the field out, "null" is a nil.
def nullable_fields(p, names)
  names.each_with_object({}) do |name, fields|
    fields[name.to_sym] = p[name] == 'null' ? nil : p[name].to_i if p.key?(name)
  end
end

def render(message)
  if message.content_type.split(';').first.strip.downcase == 'application/octet-stream'
    'b64:' + [message.body].pack('m0')
  else
    message.body_text
  end
end

def text(value)
  value.nil? ? '' : value.to_s
end

def topic_request(p)
  Taba::TopicRequest.new(
    name: p['name'], type: p['type'], fifo: flag(p, 'fifo'), prefixes: items(p, 'prefixes'),
    tags: tags_of(p, 'tags'), volatile: flag(p, 'volatile'), ttl: num(p, 'ttl'),
    max_deliveries: num(p, 'maxDeliveries'), max_message_bytes: num(p, 'maxMessageBytes'),
    idle_timeout: num(p, 'idleTimeout')
  )
end

def topic_fields(t)
  { 'id' => t.id, 'name' => t.name, 'type' => t.type, 'fifo' => text(t.fifo), 'volatile' => text(t.volatile), 'raw' => t.raw }
end

def publish_fields(r)
  { 'id' => text(r.id), 'duplicate' => text(r.duplicate), 'matched' => text(r.matched) }
end

def consume_options(p)
  { max_messages: num(p, 'max'), wait_time: num(p, 'wait'), visibility_timeout: num(p, 'visibility') }
end

def message_fields(messages)
  fields = { 'count' => messages.size.to_s, 'bodies' => messages.map { |m| render(m) }.join(',') }
  unless messages.empty?
    first = messages[0]
    fields.merge!('id' => first.id, 'receiptToken' => first.receipt_token, 'deliveryCount' => first.delivery_count.to_s,
                  'contentType' => first.content_type, 'body' => render(first))
  end
  fields
end

def publish_options(p)
  { group_id: p['groupId'], deduplication_id: p['deduplicationId'], fifo: flag(p, 'fifo') ? true : false }
end

def statuses(results)
  { 'count' => results.size.to_s, 'statuses' => results.map(&:status).join(',') }
end

# ---- the operations -------------------------------------------------------------------------------------

OPERATIONS = {
  'createTopic' => ->(c, p) { topic_fields(c.create_topic(topic_request(p))) },
  'createOrUpdateTopic' => lambda do |c, p|
    put = c.create_or_update_topic(topic_request(p), recreate: p['recreate'])
    topic_fields(put.topic).merge('outcome' => put.outcome)
  end,
  'getTopic' => ->(c, p) { topic_fields(c.get_topic(p['name'])) },
  'listTopics' => lambda do |c, p|
    topics = c.list_all_topics(name: p['name'])
    { 'count' => topics.size.to_s, 'names' => topics.map(&:name).sort.join(',') }
  end,
  'updateTopic' => lambda do |c, p|
    fields = nullable_fields(p, %w(ttl maxDeliveries maxMessageBytes)).each_with_object({}) do |(k, v), out|
      out[{ ttl: :ttl, maxDeliveries: :max_deliveries, maxMessageBytes: :max_message_bytes }[k]] = v
    end
    fields[:prefixes] = items(p, 'prefixes') if p.key?('prefixes')
    fields[:tags] = tags_of(p, 'tags') if p.key?('tags')
    fields[:idle_timeout] = num(p, 'idleTimeout') if p.key?('idleTimeout')
    topic_fields(c.update_topic(p['name'], **fields))
  end,
  'deleteTopic' => ->(c, p) { c.delete_topic(p['name'], if_empty: flag(p, 'ifEmpty') == true) && {} || {} },
  'publishText' => ->(c, p) { publish_fields(c.publish_text(p['topic'], p['body'], **publish_options(p))) },
  'publishJson' => ->(c, p) { publish_fields(c.publish_raw_json(p['topic'], p['body'], **publish_options(p))) },
  'publishBytes' => lambda do |c, p|
    options = publish_options(p).merge(content_type: p['contentType'] || 'application/octet-stream')
    publish_fields(c.publish(p['topic'], p['base64'].unpack('m').first, **options))
  end,
  # A batch of texts (one item per text); bad=true adds an item with an invalid content type.
  'publishBatch' => lambda do |c, p|
    batch = items(p, 'texts').map do |t|
      Taba::BatchMessage.new(t, content_type: 'text/plain; charset=utf-8', group_id: p['groupId'])
    end
    batch << Taba::BatchMessage.new('x', content_type: 'not a type') if flag(p, 'bad')
    statuses(c.publish_batch(p['topic'], batch))
  end,
  # Consumes and settles what came in one batch: action ack or visibility (seconds). stale=true makes the receipt of
  # the first one old before the batch: it is given back and taken again.
  'settleBatch' => lambda do |c, p|
    messages = c.consume(p['topic'], **consume_options(p))
    if flag(p, 'stale') && !messages.empty?
      c.change_visibility(p['topic'], messages[0], 0)
      c.consume(p['topic'], max_messages: 1, wait_time: 2, visibility_timeout: num(p, 'visibility'))
    end
    results = if p['action'] == 'visibility'
                c.change_visibility_batch(p['topic'], messages, p['seconds'].to_i)
              else
                c.ack_batch(p['topic'], messages)
              end
    statuses(results)
  end,
  'consume' => ->(c, p) { message_fields(c.consume(p['topic'], **consume_options(p))) },
  'ack' => ->(c, p) { c.ack(p['topic'], p['id'], p['receipt']) && {} || {} },
  'changeVisibility' => lambda do |c, p|
    r = c.change_visibility(p['topic'], p['id'], p['seconds'].to_i, p['receipt'])
    { 'id' => r.id, 'invisibleUntil' => r.invisible_until }
  end,
  # Consuming with a handler: mode ok (deleted), fail (raises), keep (delete = false) or slow (holds the message for
  # `hold` seconds, which only works if the invisibility is renewed).
  'handle' => lambda do |c, p|
    seen = []
    lost = []
    failed = false
    handled = 0
    begin
      handled = c.consume_with(p['topic'], **consume_options(p)) do |context|
        seen << render(context.message)
        raise 'the handler failed' if p['mode'] == 'fail'
        context.delete = false if p['mode'] == 'keep'
        sleep p['hold'].to_f if p['mode'] == 'slow'
        lost << context.lease_lost?
      end
    rescue Taba::Error
      raise
    rescue StandardError # the handler's own failure
      failed = true
    end
    { 'handled' => handled.to_s, 'failed' => text(failed), 'bodies' => seen.join(','), 'leaseLost' => text(lost.first == true) }
  end,
  # The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first one fail.
  'subscribe' => lambda do |c, p|
    cancel = Taba::CancelToken.new
    seen = []
    timer = Thread.new { sleep p['timeout'].to_f; cancel.cancel }
    begin
      c.subscribe(p['topic'], cancel: cancel, visibility_timeout: num(p, 'visibility')) do |message|
        seen << render(message)
        cancel.cancel if seen.size >= p['count'].to_i
        raise 'the handler failed' if p['failFirst'] == 'true' && seen.size == 1
      end
    ensure
      timer.kill
    end
    { 'count' => seen.size.to_s, 'bodies' => seen.join(',') }
  end,
  # A long polling cancelled from outside must end at once, not at the end of the wait.
  'cancelConsume' => lambda do |c, p|
    cancel = Taba::CancelToken.new
    started = Time.now
    Thread.new { sleep p['after'].to_f; cancel.cancel }
    cancelled = false
    begin
      c.consume(p['topic'], wait_time: 20, cancel: cancel)
    rescue Taba::Cancelled
      cancelled = true
    end
    { 'cancelled' => text(cancelled), 'fast' => text(Time.now - started < 10) }
  end,
  # Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
  'drain' => lambda do |c, p|
    seen = []
    deadline = Time.now + p['timeout'].to_f
    while seen.size < p['count'].to_i && Time.now < deadline
      c.consume(p['topic'], max_messages: 10, wait_time: 1, visibility_timeout: 60).each do |message|
        seen << render(message)
        c.ack(p['topic'], message)
      end
    end
    { 'count' => seen.size.to_s, 'bodies' => seen.sort.join(',') }
  end,
  'sleep' => ->(_c, p) { sleep(p['seconds'].to_f) && {} || {} }
}.freeze

# ---- the run --------------------------------------------------------------------------------------------

def check(result, expects)
  problems = []
  failure = result.key?('error')
  expects_error = expects.any? { |key, _, _| key == 'error' }
  problems << "unexpected error #{result['error']} (status #{result['status']}): #{result['message']}" if failure && !expects_error
  problems << 'expected an error, but the call succeeded' if !failure && expects_error
  expects.each do |key, op, value|
    actual = result[key]
    mismatch = if op == '~' then !actual.to_s.include?(value)
               elsif value == '*' then actual.nil? || actual.empty?
               else actual != value
               end
    problems << "#{key} #{op} #{value.inspect}, got #{actual.inspect}" if mismatch
  end
  problems
end

clients = {}
client_for = lambda do |p|
  key = [p['url'], p['token'], p['attempts']]
  clients[key] ||= begin
    options = { token: p.fetch('token', ARGS[:token]) }
    options[:max_attempts] = p['attempts'].to_i if p['attempts']
    Taba::Client.new(p.fetch('url', ARGS[:url]), **options)
  end
end

variables = {}
passed = failed = skipped = 0
ARGS[:scenario].each do |path|
  File.read(path, encoding: 'UTF-8').lines.each_with_index do |raw, n|
    line = raw.strip
    next if line.empty? || line.start_with?('#')
    where = "#{File.basename(path)}:#{n + 1}"
    op, params, expects = parse_line(line, variables)
    unless OPERATIONS.key?(op)
      skipped += 1
      puts "SKIP #{where} #{op} (not supported by this connector)"
      next
    end
    begin
      result = OPERATIONS[op].call(client_for.call(params), params)
    rescue Taba::Error => e
      result = { 'error' => e.code, 'status' => e.status.to_s, 'message' => e.to_s }
    end
    variables[params['as']] = result if params.key?('as')
    problems = check(result, expects)
    if problems.empty?
      passed += 1
      puts "PASS #{where} #{op}"
    else
      failed += 1
      puts "FAIL #{where} #{line}"
      problems.each { |problem| puts "       #{problem}" }
    end
  end
end
clients.each_value(&:close)
puts "#{ARGS[:lang]}: #{passed} passed, #{failed} failed, #{skipped} skipped"
exit(failed.zero? ? 0 : 1)
