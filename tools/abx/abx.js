// The pure part of the ABX listening tool (tools/abx/index.html).
// Plain JavaScript, no libraries, no network. In a browser these names are
// plain globals; under node the module exports them for tools/abx/abx.test.js.

// Trials in one run.
const TRIALS = 16;

// 'fail' when the listener got 12 or more right: they could hear the
// difference. Anything else, including a value that is not a number, is 'pass'.
function verdict(correct) {
  if (typeof correct !== 'number' || !Number.isFinite(correct)) return 'pass';
  return correct >= 12 ? 'fail' : 'pass';
}

// One entry per trial: 'A' or 'B', the clip X is on that trial.
// rand returns a number in [0, 1). A count below 0 or not a whole number
// gives an empty list.
function makeTrials(n, rand) {
  const count = typeof n === 'number' && Number.isInteger(n) && n > 0 ? n : 0;
  const source = typeof rand === 'function' ? rand : Math.random;
  const out = [];
  for (let i = 0; i < count; i++) {
    out.push(source() < 0.5 ? 'A' : 'B');
  }
  return out;
}

// How many answers match the trial's X, over the positions both lists have.
function scoreOf(trials, answers) {
  if (!Array.isArray(trials) || !Array.isArray(answers)) return 0;
  const len = Math.min(trials.length, answers.length);
  let score = 0;
  for (let i = 0; i < len; i++) {
    if (trials[i] === answers[i]) score++;
  }
  return score;
}

// CSV text for the log: a header, then one row per answered trial.
function logCsv(trials, answers) {
  let text = 'trial,x_was,answer\n';
  if (!Array.isArray(trials) || !Array.isArray(answers)) return text;
  const len = Math.min(trials.length, answers.length);
  for (let i = 0; i < len; i++) {
    text += (i + 1) + ',' + String(trials[i]) + ',' + String(answers[i]) + '\n';
  }
  return text;
}

if (typeof module !== 'undefined') { module.exports = { TRIALS, verdict, makeTrials, scoreOf, logCsv }; }
