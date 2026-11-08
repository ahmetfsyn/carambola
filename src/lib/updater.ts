import { ask, message } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { getCurrentWindow, ProgressBarStatus } from "@tauri-apps/api/window";
import { check } from "@tauri-apps/plugin-updater";
import { notifyUpdateStatus } from "./notifications";
import { getInstalledVersion } from "./appVersion";

let checking = false;
let lastAutomaticCheck = 0;
const AUTOMATIC_CHECK_INTERVAL = 6 * 60 * 60 * 1000;
const PENDING_UPDATE_KEY = "carambola.updater.pendingInstall";
const UPDATE_FEEDBACK_KEY = "carambola.updater.feedback";
export const UPDATE_FEEDBACK_EVENT = "carambola:update-feedback";
const UPDATE_FEEDBACK_MAX_AGE = 24 * 60 * 60 * 1000;
const PENDING_UPDATE_MAX_AGE = 30 * 60 * 1000;

export type UpdateLifecycleStatus = {
  phase: "checking" | "available" | "downloading" | "installing" | "success" | "failure" | "up-to-date";
  message: string;
  progress?: number;
};

export type UpdateFeedback = {
  phase: "success" | "failure";
  version: string | null;
  message: string;
  createdAt: number;
};

type PendingUpdate = {
  targetVersion: string;
  startedAt: number;
};

function saveUpdateFeedback(feedback: UpdateFeedback): void {
  try {
    localStorage.setItem(UPDATE_FEEDBACK_KEY, JSON.stringify(feedback));
    window.dispatchEvent(new Event(UPDATE_FEEDBACK_EVENT));
  } catch (error) {
    console.error("Could not persist update feedback:", error);
  }
}

export function getRecentUpdateFeedback(): UpdateFeedback | null {
  try {
    const raw = localStorage.getItem(UPDATE_FEEDBACK_KEY);
    if (!raw) return null;
    const feedback = JSON.parse(raw) as UpdateFeedback;
    if (
      (feedback.phase !== "success" && feedback.phase !== "failure") ||
      (feedback.version !== null && typeof feedback.version !== "string") ||
      typeof feedback.message !== "string" ||
      typeof feedback.createdAt !== "number" ||
      Date.now() - feedback.createdAt > UPDATE_FEEDBACK_MAX_AGE
    ) {
      localStorage.removeItem(UPDATE_FEEDBACK_KEY);
      return null;
    }
    return feedback;
  } catch (error) {
    console.warn("Could not read saved update feedback:", error);
    return null;
  }
}

export function clearUpdateFeedback(): void {
  try {
    localStorage.removeItem(UPDATE_FEEDBACK_KEY);
    window.dispatchEvent(new Event(UPDATE_FEEDBACK_EVENT));
  } catch (error) {
    console.warn("Could not clear saved update feedback:", error);
  }
}

function savePendingUpdate(targetVersion: string): void {
  const pending: PendingUpdate = { targetVersion, startedAt: Date.now() };
  try {
    localStorage.setItem(PENDING_UPDATE_KEY, JSON.stringify(pending));
  } catch (error) {
    // The update may still complete, but startup confirmation will be unavailable.
    console.error("Could not persist the pending update version:", error);
  }
}

function clearPendingUpdate(): void {
  try {
    localStorage.removeItem(PENDING_UPDATE_KEY);
  } catch (error) {
    console.warn("Could not clear the pending update marker:", error);
  }
}

async function sendUpdateNotification(title: string, body: string): Promise<void> {
  const submitted = await notifyUpdateStatus(title, body);
  if (!submitted) console.warn("Update feedback remains available in Settings; system notification was not submitted.");
}

async function reportPendingUpdateResult(): Promise<void> {
  let pending: PendingUpdate | null = null;
  try {
    const raw = localStorage.getItem(PENDING_UPDATE_KEY);
    if (raw) pending = JSON.parse(raw) as PendingUpdate;
  } catch (error) {
    console.warn("Could not read the pending update marker:", error);
    clearPendingUpdate();
    return;
  }
  if (!pending) return;
  if (
    typeof pending.targetVersion !== "string" ||
    typeof pending.startedAt !== "number" ||
    Date.now() - pending.startedAt > PENDING_UPDATE_MAX_AGE
  ) {
    clearPendingUpdate();
    console.info("Ignoring an expired update completion marker.");
    return;
  }

  let currentVersion: string;
  try {
    currentVersion = await getInstalledVersion();
  } catch (error) {
    console.error("Could not confirm the installed version after update:", error);
    return;
  }
  clearPendingUpdate();

  const succeeded = currentVersion === pending.targetVersion;
  const feedback: UpdateFeedback = succeeded
    ? {
        phase: "success",
        version: currentVersion,
        message: `Update complete. Carambola is now running version ${currentVersion}.`,
        createdAt: Date.now(),
      }
    : {
        phase: "failure",
        version: currentVersion,
        message: `Update to ${pending.targetVersion} did not complete. Carambola is still on version ${currentVersion}. Try again from Settings.`,
        createdAt: Date.now(),
      };
  saveUpdateFeedback(feedback);
  await sendUpdateNotification(
    succeeded ? "Carambola updated" : "Carambola update incomplete",
    feedback.message
  );
}

function safeFailureMessage(error: unknown): string {
  const detail = String(error).toLowerCase();
  if (/signature|verify|verification/.test(detail)) {
    return "Update could not be verified. Your installed version is unchanged. Try again or contact support.";
  }
  if (/timeout|network|connect|request|fetch|dns|http/.test(detail)) {
    return "Update service unavailable. Your installed version is unchanged. Check your connection and retry.";
  }
  return "Update failed. Your installed version is unchanged. Retry from Settings.";
}

export async function checkForUpdates(
  manual = false,
  onStatus?: (status: UpdateLifecycleStatus) => void
): Promise<string> {
  if (!/Win|Mac|Linux/i.test(navigator.platform)) {
    const unsupported = "In-app updates are currently available on Windows, macOS, and Linux AppImage.";
    if (manual) await message(unsupported, { title: "Carambola Updates", kind: "info" });
    return unsupported;
  }
  if (checking) return "An update check is already in progress.";
  let persistedCheck = 0;
  try {
    persistedCheck = Number(localStorage.getItem("carambola.updater.lastCheck") || 0);
  } catch (error) {
    console.warn("Could not read the automatic update check time:", error);
  }
  if (Number.isFinite(persistedCheck)) {
    lastAutomaticCheck = Math.max(lastAutomaticCheck, persistedCheck);
  }
  if (!manual && Date.now() - lastAutomaticCheck < AUTOMATIC_CHECK_INTERVAL) {
    return "Automatic update check was recently completed.";
  }

  checking = true;
  let installCompleted = false;
  let targetVersion: string | null = null;
  let currentVersion: string | null = null;
  const publish = (status: UpdateLifecycleStatus) => onStatus?.(status);
  try {
    publish({ phase: "checking", message: "Checking for updates…" });
    try {
      currentVersion = await getInstalledVersion();
    } catch {
      // The updater can still check; the status will say the version is unavailable.
    }
    const update = await check();
    if (!manual) {
      lastAutomaticCheck = Date.now();
      try {
        localStorage.setItem("carambola.updater.lastCheck", String(lastAutomaticCheck));
      } catch (error) {
        console.warn("Could not persist the automatic update check time:", error);
      }
    }
    if (!update) {
      const status = currentVersion
        ? `You're up to date (version ${currentVersion}).`
        : "You're up to date.";
      publish({ phase: "up-to-date", message: status });
      if (manual) await message(status, { title: "Carambola Updates", kind: "info" });
      return status;
    }

    const availableVersion = update.version;
    targetVersion = availableVersion;
    publish({ phase: "available", message: `Version ${availableVersion} is available.` });
    const notes = update.body?.trim();
    const description = `Carambola ${availableVersion} is available.${notes ? `\n\n${notes}` : ""}\n\nInstall it now?`;
    const accepted = await ask(description, {
      title: "Carambola Update Available",
      kind: "info",
      okLabel: "Update now",
      cancelLabel: "Later",
    });
    if (!accepted) {
      const deferred = `Update ${availableVersion} deferred.`;
      publish({ phase: "available", message: deferred });
      return deferred;
    }
    savePendingUpdate(availableVersion);
    publish({ phase: "downloading", message: `Downloading update ${availableVersion}…` });
    if (!manual) {
      await sendUpdateNotification("Carambola update started", `Downloading and installing version ${availableVersion}.`);
    }

    let downloaded = 0;
    let contentLength: number | undefined;
    let lastProgressUpdate = 0;
    let lastReportedPercent = -1;
    const appWindow = getCurrentWindow();
    const setDownloadProgress = (progress: number, status = ProgressBarStatus.Normal) => {
      const now = Date.now();
      if (progress < 100 && now - lastProgressUpdate < 100) return;
      lastProgressUpdate = now;
      void appWindow
        .setProgressBar({ status, progress })
        .catch((err) => console.warn("Could not display update progress:", err));
    };

    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        downloaded = 0;
        contentLength = event.data.contentLength;
        const messageText = contentLength
          ? `Downloading update ${availableVersion}…`
          : `Downloading update ${availableVersion}… Progress is unavailable.`;
        publish({ phase: "downloading", message: messageText });
        setDownloadProgress(0, contentLength ? ProgressBarStatus.Normal : ProgressBarStatus.Indeterminate);
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
      }

      if (event.event === "Finished") {
        publish({ phase: "installing", message: `Download complete. Installing version ${availableVersion}…` });
        setDownloadProgress(100);
      } else if (contentLength && contentLength > 0) {
        const progress = Math.min(100, Math.floor((downloaded / contentLength) * 100));
        setDownloadProgress(progress);
        if (progress !== lastReportedPercent) {
          lastReportedPercent = progress;
          publish({ phase: "downloading", message: `Downloading update ${availableVersion}… ${progress}%`, progress });
        }
      }
    });
    installCompleted = true;
    await appWindow.setProgressBar({ status: ProgressBarStatus.None });

    if (/Mac|Linux/i.test(navigator.platform)) {
      publish({ phase: "installing", message: "Update installed. Restarting Carambola…" });
      await relaunch();
      return `Carambola ${availableVersion} installed.`;
    }
    // The Windows updater exits this process after it successfully launches NSIS.
    // The next launch validates the installed version against this marker.
    publish({ phase: "installing", message: `Installer started for version ${availableVersion}.` });
    return `Installing Carambola ${availableVersion}…`;
  } catch (err) {
    try {
      await getCurrentWindow().setProgressBar({ status: ProgressBarStatus.None });
    } catch (progressError) {
      console.warn("Could not clear update progress:", progressError);
    }

    if (installCompleted && targetVersion) {
      const restartMessage = `Version ${targetVersion} was installed, but Carambola could not restart automatically. Close and reopen the app to finish the update.`;
      const feedback: UpdateFeedback = {
        phase: "success",
        version: targetVersion,
        message: restartMessage,
        createdAt: Date.now(),
      };
      saveUpdateFeedback(feedback);
      publish({ phase: "success", message: restartMessage });
      await sendUpdateNotification("Carambola update installed", restartMessage);
      if (manual) await message(restartMessage, { title: "Carambola Updates", kind: "info" });
      return restartMessage;
    }

    clearPendingUpdate();
    console.error("Updater operation failed:", err);
    const failureMessage = safeFailureMessage(err);
    const status = currentVersion
      ? `${failureMessage} Current version: ${currentVersion}.`
      : `${failureMessage} The current version could not be read.`;
    publish({ phase: "failure", message: status });
    if (manual) {
      await message(status, { title: "Carambola Update Failed", kind: "error" });
    } else {
      const feedback: UpdateFeedback = {
        phase: "failure",
        version: currentVersion,
        message: status,
        createdAt: Date.now(),
      };
      saveUpdateFeedback(feedback);
      await sendUpdateNotification("Carambola update failed", status);
    }
    return `Update check failed: ${failureMessage}`;
  } finally {
    checking = false;
  }
}

export function startAutomaticUpdateCheck(): () => void {
  if (!/Win|Mac|Linux/i.test(navigator.platform)) return () => {};

  void reportPendingUpdateResult();
  const startupTimer = window.setTimeout(() => {
    void checkForUpdates(false);
  }, 2500);
  const periodicTimer = window.setInterval(() => {
    void checkForUpdates(false);
  }, AUTOMATIC_CHECK_INTERVAL);

  return () => {
    window.clearTimeout(startupTimer);
    window.clearInterval(periodicTimer);
  };
}
