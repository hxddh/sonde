// A command as sonde shows it: its program's name for lists, its line with
// every secret hidden, and whether the user ran it at all (not sonde's own
// "replay of …"). No interface here: the app, the shared page and the
// tests read commands through these.

const SHELL = new Set(["for", "while", "until", "if", "case", "sh", "bash", "zsh", "sudo", "(", "{"]);
// steps that set the stage for the program rather than being it
const STAGE = new Set(["cd", "sleep", "echo", "export", "set", "source", ".", "true", "time", "nice", "exec", "printf", "wait", "nohup"]);

const ASSIGNMENT = /^[A-Za-z_][A-Za-z0-9_]*=/;

/** A command's shell words as typed, quotes kept (`TOKEN="ab cd"` is one
 * word), and `;`, `&&`, `||`, `|`, `&` as words of their own: as sonde's
 * own reading of a command (program.rs) has them. */
function shellWords(command: string): string[] {
  const words: string[] = [];
  let word = "";
  let quote: string | null = null;
  const flush = () => {
    if (word) words.push(word);
    word = "";
  };
  for (let i = 0; i < command.length; i++) {
    const c = command[i];
    if (quote) {
      word += c;
      if (c === "\\" && quote === '"' && i + 1 < command.length) word += command[++i];
      else if (c === quote) quote = null;
    } else if (c === "'" || c === '"') {
      quote = c;
      word += c;
    } else if (c === "\\" && /[\s\\'";|&$`]/.test(command[i + 1] ?? "")) {
      // an escape only before what it escapes: C:\Python\x.exe is a path
      word += c + (command[++i] ?? "");
    } else if (c === ";" || c === "|" || c === "&") {
      flush();
      words.push(c !== ";" && command[i + 1] === c ? c + command[++i] : c);
    } else if (/\s/.test(c)) flush();
    else word += c;
  }
  flush();
  return words;
}

const SEPARATOR = new Set([";", "&&", "||", "|", "&"]);

/** A word with any secret in it hidden: a variable's value, an option that
 * names a secret (`--password=…`), and both inside a quoted command given
 * to a shell (`sh -c 'KEY=… python x.py'`). */
function scrubWord(word: string): string {
  const eq = word.indexOf("=");
  if (eq > 0) {
    const name = word.slice(0, eq);
    if (ASSIGNMENT.test(word)) return `${name}=…`;
    if (name.startsWith("-") && /secret|password|passwd|token|key|credential/i.test(name)) return `${name}=…`;
  }
  for (const q of ["'", '"']) {
    if (word.length >= 2 && word.startsWith(q) && word.endsWith(q) && word.includes("=")) return `${q}${scrubCommand(word.slice(1, -1))}${q}`;
  }
  return word;
}

/** sonde's own words where a command would be: a replay, a watch of logs, an import. */
const OWN_WORDS = ["replay of ", "logs of ", "imported from "];

/** A command the user ran — not sonde's own `replay of …`, `logs of …` or `imported from …`. */
export function ranCommand(command?: string | null): boolean {
  const c = command?.trim();
  return !!c && !OWN_WORDS.some((w) => c.startsWith(w));
}

/** The session's title line: `$ command` for a run, sonde's own words otherwise. */
export function commandLine(command: string): string {
  return ranCommand(command) ? `$ ${scrubCommand(command)}` : command;
}

/** The command with the value of every variable it sets hidden
 * (`AWS_SECRET_ACCESS_KEY=…`), quoted values whole, the rest as typed: what
 * a list, a tooltip or a notification shows. */
export function scrubCommand(command: string): string {
  let out = "";
  let i = 0;
  const text = command.trim();
  while (i < text.length) {
    // a word starts here: an assignment's value runs to the word's end, quotes included
    if (/\s/.test(text[i]) || ";|&".includes(text[i])) {
      out += text[i++];
      continue;
    }
    let j = i;
    let quote: string | null = null;
    while (j < text.length) {
      const c = text[j];
      if (quote) {
        if (c === "\\" && quote === '"') j++;
        else if (c === quote) quote = null;
      } else if (c === "'" || c === '"') quote = c;
      else if (c === "\\") j++;
      else if (/\s/.test(c) || ";|&".includes(c)) break;
      j++;
    }
    out += scrubWord(text.slice(i, j));
    i = j;
  }
  return out;
}

/** What a command runs, for lists: its program's name (past the variables
 * it sets, `env` and its options, never one of their values; of a compound
 * command the first that is not a step setting the stage), or the command
 * itself, scrubbed, when it starts with the shell's own words. */
export function programOf(command: string | null | undefined): string | null {
  if (!command) return null;
  const words = shellWords(command);
  let first: string | null = null;
  let part: string[] = [];
  const parts: string[][] = [];
  for (const w of words) {
    if (SEPARATOR.has(w)) {
      parts.push(part);
      part = [];
    } else part.push(w);
  }
  parts.push(part);
  for (const p of parts) {
    let i = 0;
    while (i < p.length) {
      if (ASSIGNMENT.test(p[i])) i++;
      else if (p[i] === "env") {
        i++;
        while (i < p.length && (ASSIGNMENT.test(p[i]) || p[i].startsWith("-"))) i += ["-u", "-C", "--unset", "--chdir"].includes(p[i]) ? 2 : 1;
      } else break;
    }
    const w = p[i];
    if (!w) continue;
    if (SHELL.has(w)) {
      const c = scrubCommand(command);
      return c.length > 40 ? `${c.slice(0, 40)}…` : c;
    }
    const name = w.replace(/^['"]|['"]$/g, "").split(/[\\/]/).pop() || null;
    if (name && !STAGE.has(name)) return name;
    first ??= name;
  }
  return first;
}
