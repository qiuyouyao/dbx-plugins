const SETS = {
  lower: "abcdefghijklmnopqrstuvwxyz",
  upper: "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
  digits: "0123456789",
  symbols: "!@#$%^&*()-_=+[]{};:,.?/",
};
const AMBIGUOUS = new Set(["O", "0", "I", "l", "1", "|", "`", "\"", "'"]);

export class PasswordToolError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "PasswordToolError";
    this.code = code;
  }
}

function clean(chars, excludeAmbiguous) {
  return excludeAmbiguous ? [...chars].filter((char) => !AMBIGUOUS.has(char)).join("") : chars;
}

export function selectedSets(options = {}) {
  const flags = {
    lower: options.lower !== false,
    upper: options.upper !== false,
    digits: options.digits !== false,
    symbols: !!options.symbols,
  };
  const exclude = !!options.excludeAmbiguous;
  return Object.keys(SETS)
    .filter((key) => flags[key])
    .map((key) => clean(SETS[key], exclude))
    .filter((chars) => chars.length > 0);
}

export function buildAlphabet(options = {}) {
  return [...new Set(selectedSets(options).join(""))].join("");
}

function defaultRandomUint32() {
  if (!globalThis.crypto?.getRandomValues) throw new PasswordToolError("noCrypto", "Web Crypto unavailable");
  return globalThis.crypto.getRandomValues(new Uint32Array(1))[0];
}

function unbiasedIndex(max, random) {
  const limit = Math.floor(0x100000000 / max) * max;
  let value = 0;
  do {
    value = random() >>> 0;
  } while (value >= limit);
  return value % max;
}

function pick(chars, random) {
  return chars[unbiasedIndex(chars.length, random)];
}

export function generatePassword(options = {}, random = defaultRandomUint32) {
  const sets = selectedSets(options);
  if (sets.length === 0) throw new PasswordToolError("noCharset", "No character type selected");
  const length = Math.max(4, Math.min(128, Number(options.length) || 20));
  if (length < sets.length) throw new PasswordToolError("lengthTooSmall", "Length is too small for the selected types");
  const alphabet = sets.join("");
  const chars = sets.map((set) => pick(set, random));
  while (chars.length < length) chars.push(pick(alphabet, random));
  for (let i = chars.length - 1; i > 0; i -= 1) {
    const j = unbiasedIndex(i + 1, random);
    [chars[i], chars[j]] = [chars[j], chars[i]];
  }
  return chars.join("");
}

export function estimateEntropy(options = {}) {
  const alphabet = buildAlphabet(options);
  const length = Math.max(4, Math.min(128, Number(options.length) || 20));
  return alphabet ? Math.round(length * Math.log2(alphabet.length)) : 0;
}
