import React, { useState, useEffect } from "react";
import { Keyboard, Check, X, AlertCircle } from "lucide-react";
import { getGlobalShortcut, setGlobalShortcut } from "../../lib/ipc/commands";

interface ShortcutModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaved: (newShortcut: string) => void;
}

const PRESETS = [
  { label: "Cmd/Ctrl + Shift + X (Default)", value: "CommandOrControl+Shift+X" },
  { label: "Alt + A (Lightshot Style)", value: "Alt+A" },
  { label: "Cmd/Ctrl + Shift + S", value: "CommandOrControl+Shift+S" },
  { label: "PrintScreen (Windows)", value: "PrintScreen" },
  { label: "F10 Function Key", value: "F10" },
  { label: "Ctrl + Alt + S", value: "Control+Alt+S" },
];

export const ShortcutModal: React.FC<ShortcutModalProps> = ({
  isOpen,
  onClose,
  onSaved,
}) => {
  const [currentShortcut, setCurrentShortcut] = useState<string>("");
  const [selectedShortcut, setSelectedShortcut] = useState<string>("");
  const [customInput, setCustomInput] = useState<string>("");
  const [isSaving, setIsSaving] = useState<boolean>(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen) {
      getGlobalShortcut().then((sc) => {
        setCurrentShortcut(sc);
        setSelectedShortcut(sc);
        setCustomInput(sc);
        setErrorMessage(null);
      });
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleSave = async () => {
    const toSave = customInput.trim() || selectedShortcut;
    if (!toSave) {
      setErrorMessage("Please enter or select a shortcut.");
      return;
    }

    setIsSaving(true);
    setErrorMessage(null);
    try {
      await setGlobalShortcut(toSave);
      setIsSaving(false);
      onSaved(toSave);
      onClose();
    } catch (err: any) {
      setIsSaving(false);
      setErrorMessage(typeof err === "string" ? err : "Failed to register shortcut");
    }
  };

  const formatBadge = (shortcut: string) => {
    return shortcut
      .replace("CommandOrControl", "⌘ / Ctrl")
      .replace("Command", "⌘")
      .replace("Control", "Ctrl")
      .replace("Shift", "⇧")
      .replace("Alt", "⌥ / Alt")
      .replace(/\+/g, " + ");
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="w-full max-w-md bg-slate-900 border border-slate-700/80 rounded-2xl shadow-2xl p-6 text-slate-200 toolbar-fade-in"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Modal Header */}
        <div className="flex items-center justify-between pb-4 border-b border-slate-800">
          <div className="flex items-center gap-2.5">
            <div className="p-2 bg-carambola-cyan/10 text-carambola-cyan rounded-xl border border-carambola-cyan/20">
              <Keyboard size={20} />
            </div>
            <div>
              <h2 className="text-base font-bold text-white">Capture Shortcut</h2>
              <p className="text-xs text-slate-400">Configure global activation key</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-white transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Current Active Shortcut Badge */}
        <div className="my-4 p-3 rounded-xl bg-slate-950/70 border border-slate-800 flex items-center justify-between">
          <span className="text-xs text-slate-400">Current Shortcut:</span>
          <span className="px-2.5 py-1 rounded-lg bg-carambola-cyan/20 text-carambola-cyan font-mono text-xs font-semibold border border-carambola-cyan/30">
            {formatBadge(currentShortcut || "...")}
          </span>
        </div>

        {/* Presets List */}
        <div className="space-y-2 mb-4">
          <label className="text-xs font-semibold text-slate-300 block">
            Popular Presets:
          </label>
          <div className="grid grid-cols-1 gap-1.5 max-h-44 overflow-y-auto pr-1">
            {PRESETS.map((p) => {
              const isSelected = selectedShortcut === p.value;
              return (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => {
                    setSelectedShortcut(p.value);
                    setCustomInput(p.value);
                  }}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-xl text-left text-xs transition-colors border ${
                    isSelected
                      ? "bg-carambola-cyan/15 border-carambola-cyan text-white font-medium"
                      : "bg-slate-800/40 border-slate-700/50 hover:bg-slate-800 hover:border-slate-600 text-slate-300"
                  }`}
                >
                  <span>{p.label}</span>
                  <span className="font-mono text-[11px] text-slate-400">
                    {p.value}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        {/* Custom Shortcut Input */}
        <div className="space-y-1.5 mb-5">
          <label className="text-xs font-semibold text-slate-300 block">
            Custom Shortcut Definition:
          </label>
          <input
            type="text"
            value={customInput}
            onChange={(e) => {
              setCustomInput(e.target.value);
              setSelectedShortcut("");
            }}
            placeholder="e.g. CommandOrControl+Shift+X or Alt+A"
            className="w-full px-3 py-2 rounded-xl bg-slate-950 border border-slate-700 focus:border-carambola-cyan text-sm font-mono text-white placeholder-slate-500 outline-none transition-colors"
          />
          <p className="text-[11px] text-slate-500">
            Supported modifiers: <code>CommandOrControl</code>, <code>Alt</code>, <code>Shift</code>, <code>Control</code>.
          </p>
        </div>

        {/* Error Notification */}
        {errorMessage && (
          <div className="mb-4 p-2.5 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400 text-xs flex items-center gap-2">
            <AlertCircle size={16} className="shrink-0" />
            <span>{errorMessage}</span>
          </div>
        )}

        {/* Action Buttons */}
        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-800">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded-xl text-xs font-medium text-slate-300 hover:bg-slate-800 transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={isSaving}
            onClick={handleSave}
            className="px-4 py-2 rounded-xl text-xs font-semibold bg-carambola-cyan hover:bg-cyan-400 text-slate-950 transition-colors flex items-center gap-1.5 shadow-sm disabled:opacity-50"
          >
            <Check size={14} className="stroke-[3]" />
            <span>{isSaving ? "Saving..." : "Save Shortcut"}</span>
          </button>
        </div>
      </div>
    </div>
  );
};

