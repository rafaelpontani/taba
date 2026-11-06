# The Ruby client of Taba (https://github.com/rafaelpontani/taba): the nine operations of the client API,
# +subscribe+, consuming with a handler and the call retries of the common contract. No dependencies.
#
#   client = Taba::Client.new('http://localhost:8080', token: token)
#   client.publish_json('orders', order: 9001)
#   client.consume_with('orders', wait_time: 20) { |context| puts context.message.body_text }
$LOAD_PATH.unshift(File.expand_path('..', __FILE__)) unless $LOAD_PATH.include?(File.expand_path('..', __FILE__))

require 'taba/version'
require 'taba/error'
require 'taba/cancel_token'
require 'taba/config'
require 'taba/json_fields'
require 'taba/options'
require 'taba/models'
require 'taba/transport'
require 'taba/calls'
require 'taba/client'
