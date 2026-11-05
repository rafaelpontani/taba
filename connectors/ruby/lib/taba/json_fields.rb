require 'json'
require 'taba/error'

module Taba
  # The minimal JSON reader (doc 21, "No dependencies"): walks the text once and delivers the fields of an object as
  # name => text fragment and the items of an array as text fragments, without interpreting them. That keeps the
  # message +body+ exactly as the server sent it (+JSON.parse+ would turn +1.0+ into a Float and reformat the text).
  class JsonError < StandardError; end

  module JsonText
    WHITESPACE = [0x20, 0x09, 0x0d, 0x0a].freeze
    QUOTE = 0x22
    BACKSLASH = 0x5c
    OPEN = [0x7b, 0x5b].freeze   # { [
    CLOSE = [0x7d, 0x5d].freeze  # } ]
    DELIMITERS = [0x2c, 0x5d, 0x7d, 0x20, 0x09, 0x0d, 0x0a].freeze # , ] } and the whitespace

    module_function

    # The text as bytes: structural characters are ASCII, so multibyte characters need no care while scanning.
    def bytes(text)
      text.dup.force_encoding(Encoding::BINARY)
    end

    def skip(s, i)
      i += 1 while i < s.bytesize && WHITESPACE.include?(s.getbyte(i))
      i
    end

    def scan_string(s, i)
      i += 1
      n = s.bytesize
      while i < n
        c = s.getbyte(i)
        if c == BACKSLASH
          i += 1
        elsif c == QUOTE
          return i + 1
        end
        i += 1
      end
      raise JsonError, 'unterminated string'
    end

    # The index right after the value that starts at +i+ (iterative: deep nesting does not use the stack).
    def scan_value(s, i)
      n = s.bytesize
      raise JsonError, 'value expected' if i >= n
      first = s.getbyte(i)
      return scan_string(s, i) if first == QUOTE
      if OPEN.include?(first)
        depth = 0
        while i < n
          c = s.getbyte(i)
          if c == QUOTE
            i = scan_string(s, i) - 1
          elsif OPEN.include?(c)
            depth += 1
          elsif CLOSE.include?(c)
            depth -= 1
            return i + 1 if depth == 0
          end
          i += 1
        end
        raise JsonError, 'unterminated object or array'
      end
      start = i
      i += 1 while i < n && !DELIMITERS.include?(s.getbyte(i))
      raise JsonError, 'value expected' if i == start
      i
    end

    def fragment(s, from, to)
      s.byteslice(from, to - from).force_encoding(Encoding::UTF_8)
    end

    # Decodes a string fragment (with the quotes) into text.
    def decode_string(fragment)
      raise JsonError, 'string expected' unless fragment.start_with?('"')
      value = ::JSON.parse(fragment, quirks_mode: true)
      raise JsonError, 'invalid string' unless value.is_a?(String)
      value
    rescue ::JSON::ParserError
      raise JsonError, 'invalid string'
    end

    # The items of an array fragment, each as a text fragment.
    def items(text)
      s = bytes(text)
      i = skip(s, 0)
      raise JsonError, 'array expected' if i >= s.bytesize || s.getbyte(i) != 0x5b
      items = []
      i = skip(s, i + 1)
      return items if i < s.bytesize && s.getbyte(i) == 0x5d
      loop do
        i = skip(s, i)
        stop = scan_value(s, i)
        items << fragment(s, i, stop)
        i = skip(s, stop)
        if i < s.bytesize && s.getbyte(i) == 0x2c
          i += 1
        elsif i < s.bytesize && s.getbyte(i) == 0x5d
          return items
        else
          raise JsonError, '"," or "]" expected'
        end
      end
    end
  end

  # The fields of one JSON object.
  class JsonFields
    def self.parse(text)
      s = JsonText.bytes(text)
      n = s.bytesize
      fields = {}
      i = JsonText.skip(s, 0)
      raise JsonError, 'object expected' if i >= n || s.getbyte(i) != 0x7b
      i = JsonText.skip(s, i + 1)
      raise JsonError, 'unterminated object' if i >= n
      if s.getbyte(i) != 0x7d
        loop do
          i = JsonText.skip(s, i)
          raise JsonError, 'field name expected' if i >= n || s.getbyte(i) != 0x22
          key_end = JsonText.scan_string(s, i)
          key = JsonText.decode_string(JsonText.fragment(s, i, key_end))
          i = JsonText.skip(s, key_end)
          raise JsonError, '":" expected' if i >= n || s.getbyte(i) != 0x3a
          i = JsonText.skip(s, i + 1)
          stop = JsonText.scan_value(s, i)
          fields[key] = JsonText.fragment(s, i, stop)
          i = JsonText.skip(s, stop)
          if i < n && s.getbyte(i) == 0x2c
            i += 1
          elsif i < n && s.getbyte(i) == 0x7d
            break
          else
            raise JsonError, '"," or "}" expected'
          end
        end
      end
      raise JsonError, 'unexpected text after the object' if JsonText.skip(s, i + 1) != n
      new(fields)
    end

    def initialize(fields)
      @fields = fields
    end

    # The field's text fragment, or nil when absent or +null+.
    def raw(name)
      value = @fields[name]
      value.nil? || value == 'null' ? nil : value
    end

    def string(name)
      value = raw(name)
      value.nil? ? nil : JsonText.decode_string(value)
    end

    def boolean(name)
      value = raw(name)
      return nil if value.nil?
      return value == 'true' if %w(true false).include?(value)
      raise JsonError, "field \"#{name}\" is not a boolean"
    end

    def integer(name)
      value = raw(name)
      return nil if value.nil?
      raise JsonError, "field \"#{name}\" is not an integer" unless value =~ /\A-?[0-9]+\z/
      value.to_i
    end
  end
end
