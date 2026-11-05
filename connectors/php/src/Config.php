<?php

namespace Taba;

/**
 * The client configuration (doc 21). It does not read environment variables: `TABA_*` is the prefix of the
 * server configuration; whoever uses the connector reads their own and fills this object.
 *
 *     new Config('http://localhost:8080', 'token');
 *     Config::from(array('baseUrl' => 'https://taba.example', 'token' => $t, 'requestTimeout' => 10));
 */
class Config
{
    /** Address of the client API load balancer (`http://host:port` or `https://...`). Required. */
    public $baseUrl;
    /** Goes in `Authorization: Bearer`. Without it, the call goes without the header. */
    public $token;
    /** Seconds to open the connection. */
    public $connectTimeout = 5.0;
    /** Seconds of an ordinary call. `consume` uses `waitTime + 10`. */
    public $requestTimeout = 30.0;
    /** Attempts, counting the first. */
    public $maxAttempts = 3;
    /** Goes in `User-Agent`. Add the application name. */
    public $userAgent;
    /** An own CA bundle file (a private CA, for example). Default: the system's. The certificate is always verified. */
    public $caFile;
    /** An own directory of CA certificates. */
    public $caPath;

    public function __construct($baseUrl = null, $token = null)
    {
        $this->baseUrl = $baseUrl;
        $this->token = $token;
        $this->userAgent = 'taba-php/' . Client::VERSION;
    }

    /** A Config from itself, an address or an array of the fields above. */
    public static function from($value)
    {
        if ($value instanceof Config) {
            return $value;
        }
        $config = new Config();
        if (is_string($value)) {
            $config->baseUrl = $value;
            return $config;
        }
        if (!is_array($value)) {
            throw TabaException::client('client.invalid-config', 'the configuration must be a Config, an address or an array');
        }
        foreach ($value as $key => $v) {
            if (!property_exists($config, $key)) {
                throw TabaException::client('client.invalid-config', 'unknown configuration field: ' . $key);
            }
            $config->$key = $v;
        }
        return $config;
    }
}
