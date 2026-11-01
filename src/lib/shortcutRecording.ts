export interface ShortcutKeyEvent {
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

const CODE_TO_TAURI_KEY: Record<string, string> = {
  Quote: "Quote",
  Backquote: "Backquote",
  Slash: "Slash",
  Backslash: "Backslash",
  Semicolon: "Semicolon",
  Equal: "Equal",
  Minus: "Minus",
  Comma: "Comma",
  Period: "Period",
  BracketLeft: "BracketLeft",
  BracketRight: "BracketRight",
  PrintScreen: "PrintScreen",
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "ArrowUp",
  ArrowDown: "ArrowDown",
  ArrowLeft: "ArrowLeft",
  ArrowRight: "ArrowRight",
  Escape: "Escape",
  NumpadAdd: "NumpadAdd",
  NumpadSubtract: "NumpadSubtract",
  NumpadMultiply: "NumpadMultiply",
  NumpadDivide: "NumpadDivide",
  NumpadDecimal: "NumpadDecimal",
  NumpadEnter: "NumpadEnter",
  NumpadEqual: "NumpadEqual",
};

const CHAR_TO_TAURI_KEY: Record<string, string> = {
  '"': "Quote",
  "'": "Quote",
  "`": "Backquote",
  "~": "Backquote",
  ";": "Semicolon",
  ":": "Semicolon",
  "=": "Equal",
  "+": "Equal",
  "-": "Minus",
  "_": "Minus",
  "/": "Slash",
  "?": "Slash",
  "\\": "Backslash",
  "|": "Backslash",
  ",": "Comma",
  "<": "Comma",
  ".": "Period",
  ">": "Period",
  "[": "BracketLeft",
  "{": "BracketLeft",
  "]": "BracketRight",
  "}": "BracketRight",
};

export function isModifierKey(key: string): boolean {
  return ["Control", "Shift", "Alt", "Meta", "AltGraph"].includes(key);
}

/** Uses KeyboardEvent.code for physical keys so layouts do not change bindings. */
export function parseKeyboardEventToShortcut(
  event: ShortcutKeyEvent,
  isMacOS: boolean,
): string | null {
  if (isModifierKey(event.key)) return null;

  const parts: string[] = [];
  // These modifiers are distinct native flags on macOS. CommandOrControl is
  // an alias and cannot represent a separate Control modifier there.
  if (event.metaKey) parts.push(isMacOS ? "Command" : "Super");
  if (event.ctrlKey) parts.push("Control");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");

  let key = CODE_TO_TAURI_KEY[event.code];
  if (!key && /^F\d{1,2}$/.test(event.code)) key = event.code;
  if (!key && /^Key[A-Z]$/.test(event.code)) key = event.code.slice(3);
  if (!key && /^Digit\d$/.test(event.code)) key = event.code.slice(5);
  if (!key && /^Numpad\d$/.test(event.code)) key = event.code;

  // Intl* codes describe physical layout-specific keys. Never infer their
  // identity from event.key, whose value changes with keyboard layout.
  if (!key && event.code.startsWith("Intl")) return null;

  if (!key && CHAR_TO_TAURI_KEY[event.key]) key = CHAR_TO_TAURI_KEY[event.key];
  if (!key && event.key.length === 1) key = event.key.toUpperCase();
  if (!key) return null;

  // Unmodified letters and punctuation are not valid global shortcuts.
  if (parts.length === 0) {
    return key === "PrintScreen" || /^F\d{1,2}$/.test(key) ? key : null;
  }

  parts.push(key);
  return parts.join("+");
}
