<?php
/**
 * Runs the tests:  php tests/run.php [filter]   (the filter is a part of "Class::method")
 */

require __DIR__ . '/../src/autoload.php';
require __DIR__ . '/TestCase.php';
require __DIR__ . '/support.php';

error_reporting(E_ALL);
set_error_handler(function ($severity, $message, $file, $line) {
    if (!(error_reporting() & $severity)) {
        return false; // silenced with @
    }
    throw new \ErrorException($message, 0, $severity, $file, $line);
});

$filter = isset($argv[1]) ? $argv[1] : '';
$passed = 0;
$failures = array();
foreach (glob(__DIR__ . '/*Test.php') as $file) {
    require_once $file;
}
foreach (get_declared_classes() as $class) {
    if (!is_subclass_of($class, 'Taba\Tests\TestCase')) {
        continue;
    }
    $reflection = new ReflectionClass($class);
    if ($reflection->isAbstract()) {
        continue;
    }
    foreach ($reflection->getMethods(ReflectionMethod::IS_PUBLIC) as $method) {
        if (strpos($method->name, 'test') !== 0) {
            continue;
        }
        $name = $reflection->getShortName() . '::' . $method->name;
        if ($filter !== '' && strpos($name, $filter) === false) {
            continue;
        }
        $test = new $class();
        try {
            $test->setUp();
            $test->{$method->name}();
            $passed++;
            echo '.';
        } catch (\Exception $e) {
            $failures[$name] = $e;
            echo 'F';
        }
        try {
            $test->tearDown();
        } catch (\Exception $e) {
            $failures[$name . ' (tearDown)'] = $e;
        }
    }
}
echo "\n";
foreach ($failures as $name => $e) {
    echo "FAIL $name\n    " . get_class($e) . ': ' . $e->getMessage() . "\n    at " . $e->getFile() . ':' . $e->getLine() . "\n";
}
echo $passed . ' passed, ' . count($failures) . " failed\n";
exit(count($failures) === 0 ? 0 : 1);
