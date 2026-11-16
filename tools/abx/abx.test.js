// Tests for tools/abx/abx.js. Run: node --test tools/abx/abx.test.js
// (node 22 takes a file, not a directory, for --test; or node tools/abx/abx.test.js).
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { TRIALS, verdict, makeTrials, scoreOf, logCsv } = require('./abx.js');

test('12 right answers of 16 give fail', () => {
  assert.equal(TRIALS, 16);
  assert.equal(verdict(12), 'fail');
});

test('11 right answers of 16 give pass', () => {
  assert.equal(verdict(11), 'pass');
});

test('the ends of the range', () => {
  assert.equal(verdict(0), 'pass');
  assert.equal(verdict(16), 'fail');
});

test('a value that is not a number, or below 0, gives pass', () => {
  assert.equal(verdict(-1), 'pass');
  assert.equal(verdict(NaN), 'pass');
  assert.equal(verdict('12'), 'pass');
  assert.equal(verdict(undefined), 'pass');
  assert.equal(verdict(null), 'pass');
});

test('score counts matching answers', () => {
  assert.equal(scoreOf(['A', 'B', 'A', 'B'], ['A', 'A', 'A', 'B']), 3);
  assert.equal(scoreOf([], []), 0);
});

test('score only looks at positions both lists have', () => {
  assert.equal(scoreOf(['A', 'B', 'A'], ['A']), 1);
  assert.equal(scoreOf(['A'], ['A', 'B', 'B']), 1);
});

test('score of bad input is 0 and does not throw', () => {
  assert.equal(scoreOf(null, ['A']), 0);
  assert.equal(scoreOf(['A'], undefined), 0);
});

test('makeTrials is the right length and only A or B', () => {
  const trials = makeTrials(16);
  assert.equal(trials.length, 16);
  for (const t of trials) {
    assert.ok(t === 'A' || t === 'B', t);
  }
});

test('makeTrials of 0 or a negative count is empty', () => {
  assert.deepEqual(makeTrials(0), []);
  assert.deepEqual(makeTrials(-3), []);
});

test('makeTrials of a count that is not a whole number is empty', () => {
  assert.deepEqual(makeTrials(2.5), []);
  assert.deepEqual(makeTrials(NaN), []);
});

test('makeTrials follows the random source', () => {
  assert.deepEqual(makeTrials(4, () => 0.1), ['A', 'A', 'A', 'A']);
  assert.deepEqual(makeTrials(4, () => 0.9), ['B', 'B', 'B', 'B']);
});

test('the log has a header and one row per trial', () => {
  assert.equal(logCsv(['A', 'B'], ['B', 'B']), 'trial,x_was,answer\n1,A,B\n2,B,B\n');
});

test('a 16-trial log has 17 lines', () => {
  const trials = makeTrials(16, () => 0.2);
  const answers = trials.slice();
  const lines = logCsv(trials, answers).split('\n');
  // 17 lines of text plus the empty string after the final newline.
  assert.equal(lines.length, 18);
  assert.equal(lines[lines.length - 1], '');
  assert.equal(lines[0], 'trial,x_was,answer');
});

test('the log only has rows for answered trials', () => {
  assert.equal(logCsv(['A', 'B', 'A'], ['A']), 'trial,x_was,answer\n1,A,A\n');
});
