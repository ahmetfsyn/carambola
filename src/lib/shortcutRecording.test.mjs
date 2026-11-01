import assert from "node:assert/strict";
import test from "node:test";
import { isModifierKey, parseKeyboardEventToShortcut } from "./shortcutRecording.ts";

function key(overrides = {}) {
  return {
    key: "a",
    code: "KeyA",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides,
  };
}

test("macOS records Command and Control as separate modifiers", () => {
  assert.equal(parseKeyboardEventToShortcut(key({ metaKey: true }), true), "Command+A");
  assert.equal(parseKeyboardEventToShortcut(key({ ctrlKey: true }), true), "Control+A");
  assert.equal(
    parseKeyboardEventToShortcut(key({ metaKey: true, ctrlKey: true }), true),
    "Command+Control+A",
  );
});

test("modifier combinations are serialized in stable order", () => {
  assert.equal(
    parseKeyboardEventToShortcut(
      key({ metaKey: true, ctrlKey: true, altKey: true, shiftKey: true }),
      true,
    ),
    "Command+Control+Alt+Shift+A",
  );
  assert.equal(parseKeyboardEventToShortcut(key({ ctrlKey: true, metaKey: true }), false), "Super+Control+A");
});

test("physical punctuation code wins over the translated key for Turkish layouts", () => {
  assert.equal(parseKeyboardEventToShortcut(key({ key: "ı", code: "Quote", metaKey: true }), true), "Command+Quote");
  assert.equal(parseKeyboardEventToShortcut(key({ key: "?", code: "Slash", altKey: true, shiftKey: true }), true), "Alt+Shift+Slash");
});

test("unsupported layout-specific keys are not guessed from their character", () => {
  assert.equal(parseKeyboardEventToShortcut(key({ key: "\\", code: "IntlBackslash", ctrlKey: true }), true), null);
});

test("modifier-only and bare letter presses are not recorded", () => {
  assert.equal(isModifierKey("Control"), true);
  assert.equal(parseKeyboardEventToShortcut(key({ key: "Control", code: "ControlLeft", ctrlKey: true }), true), null);
  assert.equal(parseKeyboardEventToShortcut(key(), true), null);
  assert.equal(parseKeyboardEventToShortcut(key({ key: "F10", code: "F10" }), true), "F10");
});
