require 'thread'

module Taba
  # Cancels a call or a +subscribe+ loop from another thread.
  #
  #   token = Taba::CancelToken.new
  #   Thread.new { sleep 60; token.cancel }
  #   client.subscribe('orders', cancel: token) { |message| ... }   # returns when cancelled
  #
  # Cancelling closes the connection of the call in flight, so a long polling does not wait for its timeout.
  class CancelToken
    def initialize
      @mutex = Mutex.new
      @condition = ConditionVariable.new
      @cancelled = false
      @callbacks = []
    end

    def cancel
      callbacks = @mutex.synchronize do
        @cancelled = true
        @condition.broadcast
        @callbacks.dup
      end
      callbacks.each do |callback|
        begin
          callback.call
        rescue StandardError, IOError
          nil # closing a connection that is already closed
        end
      end
      self
    end

    def cancelled?
      @mutex.synchronize { @cancelled }
    end

    # Waits up to +seconds+; true when cancelled.
    def wait(seconds)
      deadline = Process.clock_gettime(Process::CLOCK_MONOTONIC) + seconds
      @mutex.synchronize do
        until @cancelled
          left = deadline - Process.clock_gettime(Process::CLOCK_MONOTONIC)
          break if left <= 0
          @condition.wait(@mutex, left)
        end
        @cancelled
      end
    end

    def raise_if_cancelled
      raise Cancelled if cancelled?
    end

    # :nodoc:
    def attach(callback)
      already = @mutex.synchronize do
        @callbacks << callback
        @cancelled
      end
      callback.call if already
    end

    # :nodoc:
    def detach(callback)
      @mutex.synchronize { @callbacks.delete(callback) }
    end
  end
end
