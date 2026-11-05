require File.expand_path('../lib/taba/version', __FILE__)

Gem::Specification.new do |spec|
  spec.name          = 'taba-client'
  spec.version       = Taba::VERSION
  spec.authors       = ['Taba']
  spec.summary       = 'The Ruby client of Taba'
  spec.description   = 'The nine operations of the Taba client API, subscribe, consuming with a handler (with automatic ' \
                       'renewal of the invisibility) and the call retries of the common contract. No dependencies.'
  spec.homepage      = 'https://github.com/rafaelpontani/taba'
  spec.license       = 'Apache-2.0'
  spec.files         = Dir['lib/**/*.rb'] + ['README.md']
  spec.require_paths = ['lib']
  spec.required_ruby_version = '>= 2.1'
end
