<?php
/**
 * The minimal JSON reader (doc 21, "No dependencies"): walks the text once and delivers the fields of an object as
 * name => text fragment and the items of an array as text fragments, without interpreting them. That keeps the
 * message `body` exactly as the server sent it (`json_decode` would turn `1.0` into a float and reformat the text).
 */

namespace Taba;

class JsonError extends \Exception
{
}

final class Json
{
    private static function skip($s, $i)
    {
        $n = strlen($s);
        while ($i < $n && strpos(" \t\r\n", $s[$i]) !== false) {
            $i++;
        }
        return $i;
    }

    private static function scanString($s, $i)
    {
        $i++;
        $n = strlen($s);
        while ($i < $n) {
            $c = $s[$i];
            if ($c === '\\') {
                $i++;
            } elseif ($c === '"') {
                return $i + 1;
            }
            $i++;
        }
        throw new JsonError('unterminated string');
    }

    /** The index right after the value that starts at `$i` (iterative: deep nesting does not use the stack). */
    public static function scanValue($s, $i)
    {
        $n = strlen($s);
        if ($i >= $n) {
            throw new JsonError('value expected');
        }
        $first = $s[$i];
        if ($first === '"') {
            return self::scanString($s, $i);
        }
        if ($first === '{' || $first === '[') {
            $depth = 0;
            while ($i < $n) {
                $c = $s[$i];
                if ($c === '"') {
                    $i = self::scanString($s, $i) - 1;
                } elseif ($c === '{' || $c === '[') {
                    $depth++;
                } elseif ($c === '}' || $c === ']') {
                    $depth--;
                    if ($depth === 0) {
                        return $i + 1;
                    }
                }
                $i++;
            }
            throw new JsonError('unterminated object or array');
        }
        $start = $i;
        while ($i < $n && strpos(",]} \t\r\n", $s[$i]) === false) {
            $i++;
        }
        if ($i === $start) {
            throw new JsonError('value expected');
        }
        return $i;
    }

    /** Decodes a string fragment (with the quotes) into text. */
    public static function decodeString($fragment)
    {
        if ($fragment === '' || $fragment[0] !== '"') {
            throw new JsonError('string expected');
        }
        $value = json_decode($fragment);
        if (!is_string($value) || json_last_error() !== JSON_ERROR_NONE) {
            throw new JsonError('invalid string');
        }
        return $value;
    }

    /** The items of an array fragment, each as a text fragment. */
    public static function items($fragment)
    {
        $s = $fragment;
        $n = strlen($s);
        $i = self::skip($s, 0);
        if ($i >= $n || $s[$i] !== '[') {
            throw new JsonError('array expected');
        }
        $items = array();
        $i = self::skip($s, $i + 1);
        if ($i < $n && $s[$i] === ']') {
            return $items;
        }
        while (true) {
            $i = self::skip($s, $i);
            $end = self::scanValue($s, $i);
            $items[] = substr($s, $i, $end - $i);
            $i = self::skip($s, $end);
            if ($i < $n && $s[$i] === ',') {
                $i++;
            } elseif ($i < $n && $s[$i] === ']') {
                return $items;
            } else {
                throw new JsonError('"," or "]" expected');
            }
        }
    }
}

/** The fields of one JSON object. */
final class JsonFields
{
    private $fields;

    private function __construct(array $fields)
    {
        $this->fields = $fields;
    }

    public static function parse($text)
    {
        $s = $text;
        $n = strlen($s);
        $fields = array();
        $i = self::skip($s, 0);
        if ($i >= $n || $s[$i] !== '{') {
            throw new JsonError('object expected');
        }
        $i = self::skip($s, $i + 1);
        if ($i >= $n) {
            throw new JsonError('unterminated object');
        }
        if ($s[$i] !== '}') {
            while (true) {
                $i = self::skip($s, $i);
                if ($i >= $n || $s[$i] !== '"') {
                    throw new JsonError('field name expected');
                }
                $keyEnd = self::stringEnd($s, $i);
                $key = Json::decodeString(substr($s, $i, $keyEnd - $i));
                $i = self::skip($s, $keyEnd);
                if ($i >= $n || $s[$i] !== ':') {
                    throw new JsonError('":" expected');
                }
                $i = self::skip($s, $i + 1);
                $end = Json::scanValue($s, $i);
                $fields[$key] = substr($s, $i, $end - $i);
                $i = self::skip($s, $end);
                if ($i < $n && $s[$i] === ',') {
                    $i++;
                } elseif ($i < $n && $s[$i] === '}') {
                    break;
                } else {
                    throw new JsonError('"," or "}" expected');
                }
            }
        }
        if (self::skip($s, $i + 1) !== $n) {
            throw new JsonError('unexpected text after the object');
        }
        return new JsonFields($fields);
    }

    private static function skip($s, $i)
    {
        $n = strlen($s);
        while ($i < $n && strpos(" \t\r\n", $s[$i]) !== false) {
            $i++;
        }
        return $i;
    }

    private static function stringEnd($s, $i)
    {
        return Json::scanValue($s, $i);
    }

    /** The field's text fragment, or null when absent or `null`. */
    public function raw($name)
    {
        if (!array_key_exists($name, $this->fields) || $this->fields[$name] === 'null') {
            return null;
        }
        return $this->fields[$name];
    }

    public function string($name)
    {
        $value = $this->raw($name);
        return $value === null ? null : Json::decodeString($value);
    }

    public function boolean($name)
    {
        $value = $this->raw($name);
        if ($value === null) {
            return null;
        }
        if ($value === 'true' || $value === 'false') {
            return $value === 'true';
        }
        throw new JsonError('field "' . $name . '" is not a boolean');
    }

    public function integer($name)
    {
        $value = $this->raw($name);
        if ($value === null) {
            return null;
        }
        if (!preg_match('/^-?[0-9]+$/', $value)) {
            throw new JsonError('field "' . $name . '" is not an integer');
        }
        return (int) $value;
    }
}
