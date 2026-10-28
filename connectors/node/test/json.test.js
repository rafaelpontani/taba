'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { JsonError, JsonFields, arrayItems } = require('../src/json');

test('the reader delivers each field as a text fragment without reserializing', () => {
  const f = JsonFields.parse(' { "a" : 1.0 , "b":{"x":[1, 2,{"y":"}"}]} , "c":"q\\"uote\\u00e9\\ud83d\\ude00" , "d":null,"e":true } ');
  assert.equal(f.raw('a'), '1.0');
  assert.equal(f.raw('b'), '{"x":[1, 2,{"y":"}"}]}');
  assert.equal(f.string('c'), 'q"uoteé😀');
  assert.equal(f.raw('d'), null);
  assert.equal(f.raw('missing'), null);
  assert.equal(f.boolean('e'), true);
  assert.equal(f.integer('a'.replace('a', 'zz')), null);
});

test('large numbers and deep nesting are kept and do not overflow the stack', () => {
  const deep = '['.repeat(50000) + ']'.repeat(50000);
  const f = JsonFields.parse(`{"n":12345678901234567890123,"deep":${deep}}`);
  assert.equal(f.raw('n'), '12345678901234567890123');
  assert.equal(f.raw('deep').length, 100000);
});

test('array items are text fragments and empty values are accepted', () => {
  assert.deepEqual(arrayItems('[]'), []);
  assert.deepEqual(arrayItems(' [ 1 , "a,b" , {"k":[1,2]} , [ ] , null ] '), ['1', '"a,b"', '{"k":[1,2]}', '[ ]', 'null']);
  assert.equal(JsonFields.parse('{}').raw('x'), null);
  assert.equal(JsonFields.parse('{"e":""}').string('e'), '');
});

test('malformed documents are a JsonError and not a crash', () => {
  for (const bad of ['', 'x', '[1]', '{', '{"a"}', '{"a":}', '{"a":1,}', '{"a":"x', '{"a":1} extra', '{"a":[1,2}']) {
    assert.throws(() => JsonFields.parse(bad), JsonError, bad);
  }
  assert.throws(() => arrayItems('{}'), JsonError);
  assert.throws(() => JsonFields.parse('{"a":"x"}').integer('a'), JsonError);
});
