<?php
/**
 * A tiny test base (the connector has no dependencies, so no PHPUnit): assertions plus `run.php`, which finds the
 * `*Test.php` files, runs every public method that starts with `test` and fails with a non-zero exit code.
 */

namespace Taba\Tests;

class AssertionFailed extends \Exception
{
}

abstract class TestCase
{
    public function setUp()
    {
    }

    public function tearDown()
    {
    }

    protected function fail($message)
    {
        throw new AssertionFailed($message);
    }

    protected function assertSame($expected, $actual, $what = '')
    {
        if ($expected !== $actual) {
            $this->fail(($what === '' ? '' : $what . ': ') . 'expected ' . var_export($expected, true) . ', got ' . var_export($actual, true));
        }
    }

    protected function assertTrue($value, $what = '')
    {
        $this->assertSame(true, $value, $what);
    }

    protected function assertFalse($value, $what = '')
    {
        $this->assertSame(false, $value, $what);
    }

    protected function assertNull($value, $what = '')
    {
        $this->assertSame(null, $value, $what);
    }

    protected function assertContains($needle, $haystack, $what = '')
    {
        if (strpos($haystack, $needle) === false) {
            $this->fail(($what === '' ? '' : $what . ': ') . var_export($needle, true) . ' is not in ' . var_export($haystack, true));
        }
    }

    protected function assertLessThan($limit, $value, $what = '')
    {
        if (!($value < $limit)) {
            $this->fail(($what === '' ? '' : $what . ': ') . $value . ' is not less than ' . $limit);
        }
    }

    /** Runs `$call` and returns the `TabaException` it throws, with the given `errorCode`. */
    protected function assertError($code, $call)
    {
        try {
            call_user_func($call);
        } catch (\Taba\TabaException $e) {
            $this->assertSame($code, $e->errorCode, 'error code (' . $e->getMessage() . ')');
            return $e;
        }
        $this->fail('expected the error ' . $code . ', but the call succeeded');
    }

    protected function assertThrows($class, $call)
    {
        try {
            call_user_func($call);
        } catch (\Exception $e) {
            if (!($e instanceof $class)) {
                $this->fail('expected ' . $class . ', got ' . get_class($e) . ': ' . $e->getMessage());
            }
            return $e;
        }
        $this->fail('expected ' . $class . ', but nothing was thrown');
    }
}
