import React, { useState, useEffect, useRef } from "react";
import {
  Keyboard,
  Check,
  CheckCircle2,
  X,
  AlertCircle,
  ArrowUpCircle,
  Download,
  Power,
  Bell,
  RefreshCw,
} from "lucide-react";
import { isPermissionGranted } from "@tauri-apps/plugin-notification";
import {
  AutostartStatus,
  getAutostartStatus,
  getGlobalShortcut,
  getScreenshotNotificationSettings,
  setAutostartEnabled,
  setScreenshotNotifications,
  setGlobalShortcut,
  ScreenshotNotificationSettings,
} from "../../lib/ipc/commands";
import { isModifierKey, parseKeyboardEventToShortcut } from "../../lib/shortcutRecording";
import {
  checkForUpdates,
  clearUpdateFeedback,
  getRecentUpdateFeedback,
  UPDATE_FEEDBACK_EVENT,
  UpdateLifecycleStatus,
} from "../../lib/updater";
import { AboutSection } from "./AboutSection";

interface ShortcutModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaved: (newShortcut: string) => void;
}

const PRESETS = [
  { label: "PrintScreen (Windows Default)", value: "PrintScreen" },
  { label: "Cmd/Ctrl + Shift + X (macOS Default)", value: "CommandOrControl+Shift+X" },
  { label: "Alt + A (Lightshot Style)", value: "Alt+A" },
  { label: "Cmd/Ctrl + Shift + S", value: "CommandOrControl+Shift+S" },
  { label: "F10 Function Key", value: "F10" },
  { label: "Ctrl + Alt + S", value: "Control+Alt+S" },
];

const UPDATE_PHASE_LABELS: Record<UpdateLifecycleStatus["phase"], string> = {
  checking: "Checking for updates",
  available: "Update available",
  downloading: "Downloading",
  installing: "Installing",
  success: "Successfully updated",
  failure: "Update failed",
  "up-to-date": "No update available",
};

const IS_MACOS = /Mac|iPhone|iPad/.test(navigator.platform);

export const ShortcutModal: React.FC<ShortcutModalProps> = ({
  isOpen,
  onClose,
  onSaved,
}) => {
  const [currentShortcut, setCurrentShortcut] = useState<string>("");
  const [selectedShortcut, setSelectedShortcut] = useState<string>("");
  const [customInput, setCustomInput] = useState<string>("");
  const [isRecording, setIsRecording] = useState<boolean>(false);
  const [isSaving, setIsSaving] = useState<boolean>(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [autostartSaving, setAutostartSaving] = useState<boolean>(false);
  const [notifications, setNotifications] = useState<ScreenshotNotificationSettings | null>(null);
  const [notificationPermissionGranted, setNotificationPermissionGranted] = useState<boolean | null>(null);
  const [notificationSaving, setNotificationSaving] = useState<boolean>(false);
  const [notificationError, setNotificationError] = useState<string | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<UpdateLifecycleStatus | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (isOpen) {
      getGlobalShortcut().then((sc) => {
        setCurrentShortcut(sc);
        setSelectedShortcut(sc);
        setCustomInput(sc);
        setIsRecording(false);
        setErrorMessage(null);
      });
      getAutostartStatus()
        .then(setAutostart)
        .catch((err) =>
          setAutostart({
            supported: false,
            enabled: false,
            reason: `Could not read Start on Boot: ${String(err)}`,
          })
        );
      getScreenshotNotificationSettings()
        .then(setNotifications)
        .catch((err) => {
          setNotificationError(`Could not read notification preference: ${String(err)}`);
        });
      isPermissionGranted()
        .then(setNotificationPermissionGranted)
        .catch(() => setNotificationPermissionGranted(false));
    }
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    const refreshFeedback = () => {
      const feedback = getRecentUpdateFeedback();
      setUpdateStatus(feedback ? { phase: feedback.phase, message: feedback.message } : null);
    };
    refreshFeedback();
    window.addEventListener(UPDATE_FEEDBACK_EVENT, refreshFeedback);
    return () => window.removeEventListener(UPDATE_FEEDBACK_EVENT, refreshFeedback);
  }, [isOpen]);

  if (!isOpen) return null;

  const handleToggleAutostart = async () => {
    if (!autostart || !autostart.supported || autostartSaving) return;
    setAutostartSaving(true);
    try {
      const next = await setAutostartEnabled(!autostart.enabled);
      setAutostart(next);
    } catch (err) {
      setAutostart((prev) =>
        prev
          ? { ...prev, reason: typeof err === "string" ? err : "Failed to change Start on Boot" }
          : prev
      );
    } finally {
      setAutostartSaving(false);
    }
  };

  const handleToggleNotifications = async () => {
    if (!notifications || notificationSaving) return;
    setNotificationSaving(true);
    setNotificationError(null);
    try {
      const next = await setScreenshotNotifications(!notifications.enabled);
      setNotifications(next);
    } catch (err) {
      setNotificationError(
        typeof err === "string" ? err : "Failed to save screenshot notification preference"
      );
    } finally {
      setNotificationSaving(false);
    }
  };

  const handleCheckForUpdates = async () => {
    if (updateBusy) return;
    setUpdateBusy(true);
    clearUpdateFeedback();
    setUpdateStatus({ phase: "checking", message: "Checking for updates…" });
    try {
      await checkForUpdates(true, setUpdateStatus);
    } catch (err) {
      console.error("Manual update check failed:", err);
      setUpdateStatus({
        phase: "failure",
        message: "The update check failed. Check your connection and try again.",
      });
    } finally {
      setUpdateBusy(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      setIsRecording(false);
      inputRef.current?.blur();
      return;
    }

    const shortcut = parseKeyboardEventToShortcut(e, IS_MACOS);
    if (shortcut) {
      setCustomInput(shortcut);
      setSelectedShortcut(shortcut);
      setErrorMessage(null);
      setIsRecording(false);
      inputRef.current?.blur();
    } else if (!isModifierKey(e.key)) {
      setErrorMessage(
        e.code.startsWith("Intl")
          ? "This physical key is not supported by the global shortcut API. Choose another key."
          : "Use a modifier with this key. Function keys and Print Screen can be used alone."
      );
      setIsRecording(false);
      inputRef.current?.blur();
    }
  };

  const handleSave = async () => {
    // Normalize custom string if user typed double quote or single quote directly
    let toSave = (customInput.trim() || selectedShortcut);
    if (!toSave) {
      setErrorMessage("Please enter or select a shortcut.");
      return;
    }

    // Auto-normalize common punctuation representations to Tauri names
    toSave = toSave
      .replace(/\+["']$/, "+Quote")
      .replace(/\+`$/, "+Backquote");

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
      .replace("Super", IS_MACOS ? "⌘" : "⊞")
      .replace("Control", "Ctrl")
      .replace("Shift", "⇧")
      .replace("Alt", "⌥ / Alt")
      .replace("Quote", '"')
      .replace("Backquote", "`")
      .replace(/\+/g, " + ");
  };

  const statusPhase = updateStatus?.phase;
  const UpdateStatusIcon =
    statusPhase === "failure"
      ? AlertCircle
      : statusPhase === "success" || statusPhase === "up-to-date"
        ? CheckCircle2
        : statusPhase === "available"
          ? ArrowUpCircle
          : statusPhase === "downloading"
            ? Download
            : RefreshCw;
  const updateStatusTone =
    statusPhase === "failure"
      ? "border-rose-900/50 bg-rose-950/25 text-rose-300"
      : statusPhase === "success"
        ? "border-emerald-900/50 bg-emerald-950/20 text-emerald-300"
        : statusPhase === "available"
          ? "border-cyan-900/50 bg-cyan-950/20 text-carambola-cyan"
          : "border-slate-800 bg-slate-900/60 text-slate-400";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="settings-scrollbar w-full max-w-md max-h-[90vh] overflow-y-auto overscroll-contain bg-slate-900 border border-slate-700/80 rounded-2xl shadow-2xl p-6 text-slate-200 toolbar-fade-in"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Modal Header */}
        <div className="flex items-center justify-between pb-4 border-b border-slate-800">
          <div className="flex items-center gap-2.5">
            <div className="p-2 bg-carambola-cyan/10 text-carambola-cyan rounded-xl border border-carambola-cyan/20">
              <Keyboard size={20} />
            </div>
            <div>
              <h2 className="text-base font-bold text-white">Settings</h2>
              <p className="text-xs text-slate-400">Capture shortcut and startup behaviour</p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-white transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Start on Boot */}
        <div className="mt-4 p-3 rounded-xl bg-slate-950/70 border border-slate-800">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="p-2 bg-slate-800 text-slate-300 rounded-xl border border-slate-700/60">
                <Power size={16} />
              </div>
              <div>
                <div className="text-xs font-semibold text-slate-200">Start on Boot</div>
                <div className="text-[11px] text-slate-400">
                  Launch Carambola in the background at login.
                </div>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={autostart?.enabled ?? false}
              aria-label="Start on Boot"
              disabled={
                !autostart || !autostart.supported || autostartSaving || autostart === null
              }
              onClick={handleToggleAutostart}
              className={`relative w-11 h-6 rounded-full border transition-colors shrink-0 disabled:opacity-40 disabled:cursor-not-allowed ${
                autostart?.enabled
                  ? "bg-carambola-cyan border-carambola-cyan"
                  : "bg-slate-700 border-slate-600"
              }`}
            >
              <span
                className={`absolute top-0.5 rounded-full bg-white shadow-sm transition-all ${
                  autostart?.enabled ? "left-[22px]" : "left-0.5"
                }`}
                style={{ width: 18, height: 18 }}
              />
            </button>
          </div>
          {autostart?.reason && (
            <p className="mt-2 text-[11px] text-amber-400/90 flex items-start gap-1.5">
              <AlertCircle size={13} className="shrink-0 mt-0.5" />
              <span>{autostart.reason}</span>
            </p>
          )}
          {autostart && !autostart.supported && !autostart.reason && (
            <p className="mt-2 text-[11px] text-amber-400/90">
              Start on Boot is not supported on this platform.
            </p>
          )}
        </div>

        {/* Screenshot Notifications */}
        <div className="mt-3 p-3 rounded-xl bg-slate-950/70 border border-slate-800">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2.5">
              <div className="p-2 bg-slate-800 text-slate-300 rounded-xl border border-slate-700/60">
                <Bell size={16} />
              </div>
              <div>
                <div className="text-xs font-semibold text-slate-200">Screenshot Notifications</div>
                <div className="text-[11px] text-slate-400">
                  Show a system notification after successfully capturing a screenshot.
                </div>
              </div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={notifications?.enabled ?? true}
              aria-label="Screenshot Notifications"
              disabled={!notifications || notificationSaving}
              onClick={handleToggleNotifications}
              className={`relative w-11 h-6 rounded-full border transition-colors shrink-0 disabled:opacity-40 disabled:cursor-not-allowed ${
                notifications?.enabled
                  ? "bg-carambola-cyan border-carambola-cyan"
                  : "bg-slate-700 border-slate-600"
              }`}
            >
              <span
                className={`absolute top-0.5 rounded-full bg-white shadow-sm transition-all ${
                  notifications?.enabled ? "left-[22px]" : "left-0.5"
                }`}
                style={{ width: 18, height: 18 }}
              />
            </button>
          </div>
          {notifications?.enabled &&
            notifications.permission_prompted &&
            notificationPermissionGranted === false && (
              <p className="mt-2 text-[11px] text-amber-400/90 flex items-start gap-1.5">
                <AlertCircle size={13} className="shrink-0 mt-0.5" />
                <span>
                  System notification permission is off. Allow Carambola in your system notification settings; it will not ask again automatically.
                </span>
              </p>
            )}
          {notificationError && (
            <p className="mt-2 text-[11px] text-amber-400/90 flex items-start gap-1.5">
              <AlertCircle size={13} className="shrink-0 mt-0.5" />
              <span>{notificationError}</span>
            </p>
          )}
        </div>

        <div className="mt-4 text-xs font-semibold text-slate-300">Capture Shortcut</div>

        {/* Current Active Shortcut Badge */}
        <div className="my-4 p-3 rounded-xl bg-slate-950/70 border border-slate-800 flex items-center justify-between">
          <span className="text-xs text-slate-400">Current Shortcut:</span>
          <span className="px-2.5 py-1 rounded-lg bg-carambola-cyan/20 text-carambola-cyan font-mono text-xs font-semibold border border-carambola-cyan/30">
            {formatBadge(currentShortcut || "...")}
          </span>
        </div>

        {/* Custom Shortcut Input with Keyboard Listener */}
        <div className="space-y-1.5 mb-4">
          <div className="flex items-center justify-between">
            <label className="text-xs font-semibold text-slate-300 block">
              Custom Shortcut Definition:
            </label>
            {isRecording && (
              <span className="text-[11px] text-carambola-cyan animate-pulse font-medium">
                Listening for keys...
              </span>
            )}
          </div>
          <div className="relative">
            <input
              ref={inputRef}
              type="text"
              readOnly
              value={isRecording ? "Press key combination..." : (customInput ? formatBadge(customInput) : "")}
              onFocus={() => {
                setIsRecording(true);
                setErrorMessage(null);
              }}
              onBlur={() => {
                setIsRecording(false);
              }}
              onKeyDown={handleKeyDown}
              placeholder="Click here and press key combo..."
              className={`w-full px-3 py-2.5 rounded-xl bg-slate-950 border text-sm font-mono transition-colors outline-none cursor-pointer ${
                isRecording
                  ? "border-carambola-cyan text-carambola-cyan ring-2 ring-carambola-cyan/20"
                  : "border-slate-700 focus:border-carambola-cyan text-white placeholder-slate-500"
              }`}
            />
            {customInput && !isRecording && (
              <button
                type="button"
                onClick={() => {
                  setCustomInput("");
                  setSelectedShortcut("");
                  inputRef.current?.focus();
                }}
                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300 p-1"
                title="Clear"
              >
                <X size={14} />
              </button>
            )}
          </div>
          <p className="text-[11px] text-slate-400">
            Click the input field and press any key combination (e.g. <code>Alt + "</code>, <code>Alt + A</code>, or <code>PrintScreen</code>). Press <code>Esc</code> to cancel recording.
          </p>
        </div>

        {/* Presets List */}
        <div className="space-y-2 mb-4">
          <label className="text-xs font-semibold text-slate-300 block">
            Popular Presets:
          </label>
          <div className="grid grid-cols-1 gap-1.5">
            {PRESETS.map((p) => {
              const isSelected = selectedShortcut === p.value;
              return (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => {
                    setSelectedShortcut(p.value);
                    setCustomInput(p.value);
                    setIsRecording(false);
                    setErrorMessage(null);
                  }}
                  className={`w-full flex items-center justify-between px-3 py-2 rounded-xl text-left text-xs transition-colors border ${
                    isSelected
                      ? "bg-carambola-cyan/15 border-carambola-cyan text-white font-medium"
                      : "bg-slate-800/40 border-slate-700/50 hover:bg-slate-800 hover:border-slate-600 text-slate-300"
                  }`}
                >
                  <span>{p.label}</span>
                  <span className="font-mono text-[11px] text-slate-400">
                    {formatBadge(p.value)}
                  </span>
                </button>
              );
            })}
          </div>
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

        {/* Update lifecycle */}
        <div className="mt-3 p-3 rounded-xl bg-slate-950/70 border border-slate-800">
          <div className="flex items-center justify-between gap-3">
            <div className="text-xs font-semibold text-slate-200">Updates</div>
            <button
              type="button"
              onClick={handleCheckForUpdates}
              disabled={updateBusy}
              className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border border-slate-700 bg-slate-800/70 text-[11px] font-semibold text-slate-200 hover:border-carambola-cyan/50 hover:text-white disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              <RefreshCw size={12} className={updateBusy ? "animate-spin" : ""} />
              {updateBusy
                ? updateStatus?.phase === "downloading"
                  ? "Downloading…"
                  : updateStatus?.phase === "installing"
                    ? "Installing…"
                    : "Checking…"
                : updateStatus?.phase === "failure"
                  ? "Retry"
                  : "Check for Updates"}
            </button>
          </div>
          {updateStatus && (
            <div
              role="status"
              aria-live="polite"
              className={`mt-2 flex items-start gap-2 rounded-lg border px-2.5 py-2 ${updateStatusTone}`}
            >
              <UpdateStatusIcon
                size={14}
                className={`mt-0.5 shrink-0 ${
                  updateBusy && (statusPhase === "checking" || statusPhase === "installing")
                    ? "animate-spin"
                    : ""
                }`}
              />
              <div className="min-w-0">
                <div className="text-[11px] font-semibold">{UPDATE_PHASE_LABELS[updateStatus.phase]}</div>
                <p className="mt-0.5 break-words text-[11px] leading-relaxed text-slate-300">
                  {updateStatus.message}
                </p>
                {typeof updateStatus.progress === "number" && (
                  <div
                    className="mt-1.5 h-1 overflow-hidden rounded-full bg-slate-800"
                    role="progressbar"
                    aria-label="Update download progress"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={updateStatus.progress}
                  >
                    <div
                      className="h-full rounded-full bg-carambola-cyan transition-[width]"
                      style={{ width: `${updateStatus.progress}%` }}
                    />
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        <AboutSection />
      </div>
    </div>
  );
};
