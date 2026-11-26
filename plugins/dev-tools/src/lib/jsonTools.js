export class JsonToolError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "JsonToolError";
    this.code = code;
  }
}

export function parseJson(input) {
  const text = typeof input === "string" ? input.trim() : "";
  if (!text) throw new JsonToolError("empty", "Input is empty");
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new JsonToolError("invalid", error.message);
  }
}

export function formatJson(input, spaces = 2) {
  const indent = Math.max(0, Math.min(8, Number(spaces) || 2));
  return JSON.stringify(parseJson(input), null, indent);
}

export function minifyJson(input) {
  return JSON.stringify(parseJson(input));
}

const NUMBER = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;

// Colors the exact output text; it does not validate JSON. Adjacent tokens of
// the same type are merged to keep the rendered DOM small.
export function tokenizeJson(text) {
  const source = typeof text === "string" ? text : "";
  const tokens = [];
  const add = (type, value) => {
    const last = tokens[tokens.length - 1];
    if (last && last.type === type) last.text += value;
    else tokens.push({ type, text: value });
  };

  let i = 0;
  while (i < source.length) {
    const char = source[i];
    if (/\s/.test(char)) {
      const start = i;
      while (i < source.length && /\s/.test(source[i])) i += 1;
      add("plain", source.slice(start, i));
      continue;
    }
    if (char === '"') {
      const start = i;
      i += 1;
      while (i < source.length) {
        if (source[i] === "\\") {
          i += 2;
          continue;
        }
        if (source[i] === '"') {
          i += 1;
          break;
        }
        i += 1;
      }
      const raw = source.slice(start, i);
      let next = i;
      while (next < source.length && /\s/.test(source[next])) next += 1;
      add(source[next] === ":" ? "key" : "string", raw);
      continue;
    }
    if (char === "-" || (char >= "0" && char <= "9")) {
      NUMBER.lastIndex = i;
      const match = NUMBER.exec(source);
      if (match) {
        add("number", match[0]);
        i += match[0].length;
        continue;
      }
    }
    if (source.startsWith("true", i) || source.startsWith("false", i)) {
      const literal = source.startsWith("true", i) ? "true" : "false";
      add("boolean", literal);
      i += literal.length;
      continue;
    }
    if (source.startsWith("null", i)) {
      add("null", "null");
      i += 4;
      continue;
    }
    if ("{}[]:,".includes(char)) {
      add("punct", char);
      i += 1;
      continue;
    }
    add("plain", char);
    i += 1;
  }
  return tokens;
}
