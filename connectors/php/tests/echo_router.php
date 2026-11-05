<?php
/**
 * The router of the built-in PHP server the real-`curl` tests talk to. It answers with what it received, so the
 * test sees the request exactly as it went on the wire. A path that ends in `status-NNN` answers with that status
 * (and, for 503, a `Retry-After`).
 */

$body = file_get_contents('php://input');
$headers = array();
foreach (getallheaders() as $name => $value) {
    $headers[strtolower($name)] = $value;
}
$path = parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH);
$status = 200;
if (preg_match('/status-([0-9]{3})$/', $path, $m)) {
    $status = (int) $m[1];
}
http_response_code($status);
header('Content-Type: application/json');
header('X-Request-Id: echo-1');
if ($status === 503) {
    header('Retry-After: 3');
}
echo json_encode(array(
    'id' => 'echo-id',
    'name' => 'echo',
    'type' => 'queue',
    'createdAt' => 'now',
    'code' => 'echo.status',
    'message' => 'status ' . $status,
    'method' => $_SERVER['REQUEST_METHOD'],
    'uri' => $_SERVER['REQUEST_URI'],
    'headers' => $headers,
    'body' => base64_encode($body),
));
