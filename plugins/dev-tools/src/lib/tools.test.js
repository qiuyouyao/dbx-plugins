import test from "node:test";
import assert from "node:assert/strict";
import { formatJson, minifyJson, tokenizeJson, JsonToolError } from "./jsonTools.js";
import { selectedSets, generatePassword, estimateEntropy, PasswordToolError } from "./passwordTools.js";

function seeded(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state;
  };
}

test("formatJson pretty prints with two spaces", () => {
  assert.equal(formatJson('{"a":1,"b":[2,3]}'), '{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}');
});

test("formatJson honors a four-space indent", () => {
  assert.equal(formatJson('{"a":1}', 4), '{\n    "a": 1\n}');
});

test("formatJson rejects empty input", () => {
  assert.throws(() => formatJson("   "), (error) => error instanceof JsonToolError && error.code === "empty");
});

test("formatJson rejects invalid input", () => {
  assert.throws(() => formatJson("{oops}"), (error) => error instanceof JsonToolError && error.code === "invalid");
});

test("minifyJson removes insignificant whitespace", () => {
  assert.equal(minifyJson('{ "a" : 1 }'), '{"a":1}');
});

test("selectedSets honors flags and the ambiguous-character filter", () => {
  assert.deepEqual(selectedSets({ lower: true, upper: false, digits: false, symbols: false }), ["abcdefghijklmnopqrstuvwxyz"]);
  const digits = selectedSets({ lower: false, upper: false, digits: true, symbols: false, excludeAmbiguous: true });
  assert.equal(digits.length, 1);
  assert.ok(!digits[0].includes("0"));
});

test("generatePassword is deterministic with an injected RNG", () => {
  const options = { length: 4, lower: true, upper: false, digits: false, symbols: false };
  assert.equal(generatePassword(options, () => 0), "aaaa");
});

test("generatePassword respects length and selected character sets", () => {
  const options = { length: 32, lower: true, upper: true, digits: true, symbols: false };
  const password = generatePassword(options, seeded(7));
  const allowed = new Set(selectedSets(options).join(""));
  assert.equal(password.length, 32);
  assert.ok([...password].every((char) => allowed.has(char)));
});

test("generatePassword rejects an empty charset", () => {
  const options = { length: 12, lower: false, upper: false, digits: false, symbols: false };
  assert.throws(() => generatePassword(options), (error) => error instanceof PasswordToolError && error.code === "noCharset");
});

test("estimateEntropy grows with length", () => {
  const options = { length: 8, lower: true, upper: true, digits: true, symbols: false };
  assert.ok(estimateEntropy({ ...options, length: 16 }) > estimateEntropy(options));
});

test("tokenizeJson classifies keys, values and punctuation", () => {
  const tokens = tokenizeJson('{"a":1,"b":[true,null,"x"]}');
  assert.deepEqual(tokens.map((token) => token.type), [
    "punct", "key", "punct", "number", "punct", "key", "punct",
    "boolean", "punct", "null", "punct", "string", "punct",
  ]);
  assert.equal(tokens.map((token) => token.text).join(""), '{"a":1,"b":[true,null,"x"]}');
});

test("tokenizeJson preserves whitespace", () => {
  const text = '{\n  "a": 1\n}';
  assert.equal(tokenizeJson(text).map((token) => token.text).join(""), text);
});

test("tokenizeJson handles escaped quotes and exponents", () => {
  const tokens = tokenizeJson('{"a\\"b":-1.2e+3}');
  assert.equal(tokens[1].type, "key");
  assert.equal(tokens[3].type, "number");
  assert.equal(tokens[3].text, "-1.2e+3");
});
