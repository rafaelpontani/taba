<?php
/**
 * The autoloader for whoever does not use Composer: `require 'taba-php/src/autoload.php';`.
 * (With Composer, `composer.json` already maps the same folder.)
 */

spl_autoload_register(function ($class) {
    if (strpos($class, 'Taba\\') !== 0) {
        return;
    }
    static $files = array(
        'Client' => 'Client.php',
        'Config' => 'Config.php',
        'TabaException' => 'Errors.php',
        'Cancelled' => 'Errors.php',
        'CancelToken' => 'Errors.php',
        'Json' => 'Json.php',
        'JsonError' => 'Json.php',
        'JsonFields' => 'Json.php',
        'Fields' => 'Options.php',
        'ConsumeOptions' => 'Options.php',
        'PublishOptions' => 'Options.php',
        'TopicRequest' => 'Options.php',
        'TopicUpdate' => 'Options.php',
        'BatchMessage' => 'Options.php',
        'Topic' => 'Models.php',
        'Page' => 'Models.php',
        'Message' => 'Models.php',
        'DistributorDelivery' => 'Models.php',
        'PublishResponse' => 'Models.php',
        'VisibilityResponse' => 'Models.php',
        'BatchResult' => 'Models.php',
        'MessageContext' => 'Models.php',
        'Parser' => 'Models.php',
        'Calls' => 'Calls.php',
        'ApiCall' => 'Transport.php',
        'ApiReply' => 'Transport.php',
        'NetworkFailure' => 'Transport.php',
        'CurlExecutor' => 'Transport.php',
        'Transport' => 'Transport.php',
    );
    $name = substr($class, 5);
    if (isset($files[$name])) {
        require_once __DIR__ . '/' . $files[$name];
    }
});
