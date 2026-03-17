import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import type { AppUpdateCheckSource, AppUpdateInfo } from "../types";

const LATEST_RELEASE_API =
  "https://api.github.com/repos/Coang2623/codex-quota-monitor/releases/latest";
const LATEST_RELEASE_PAGE =
  "https://github.com/Coang2623/codex-quota-monitor/releases/latest";

interface GithubLatestReleaseResponse {
  tag_name?: string;
  name?: string;
  html_url?: string;
  published_at?: string;
  body?: string;
  draft?: boolean;
  prerelease?: boolean;
}

const initialUpdateState: AppUpdateInfo = {
  status: "idle",
  current_version: null,
  latest_version: null,
  release_name: null,
  release_url: null,
  published_at: null,
  body: null,
  error: null,
  checked_at: null,
  source: null,
  can_download_and_install: false,
  downloaded_bytes: 0,
  content_length: null,
  download_percent: null,
};

function normalizeVersion(version: string | null | undefined): string | null {
  if (!version) return null;
  const trimmed = version.trim();
  if (!trimmed) return null;
  return trimmed.replace(/^v/i, "");
}

function compareVersions(currentVersion: string, latestVersion: string): number {
  const currentParts = currentVersion.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const latestParts = latestVersion.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const maxLength = Math.max(currentParts.length, latestParts.length);

  for (let index = 0; index < maxLength; index += 1) {
    const currentPart = currentParts[index] ?? 0;
    const latestPart = latestParts[index] ?? 0;
    if (currentPart !== latestPart) {
      return currentPart < latestPart ? -1 : 1;
    }
  }

  return 0;
}

async function fetchLatestGithubRelease(): Promise<GithubLatestReleaseResponse> {
  const response = await fetch(LATEST_RELEASE_API, {
    headers: {
      Accept: "application/vnd.github+json",
    },
  });

  if (!response.ok) {
    throw new Error(`GitHub release check failed with status ${response.status}`);
  }

  const release = (await response.json()) as GithubLatestReleaseResponse;
  if (release.draft || release.prerelease) {
    throw new Error("Latest GitHub release is not a stable production release");
  }

  return release;
}

function computeProgressPercent(downloadedBytes: number, contentLength: number | null): number | null {
  if (!contentLength || contentLength <= 0) {
    return null;
  }

  return Math.max(0, Math.min(100, Math.round((downloadedBytes / contentLength) * 100)));
}

export function useAppUpdate() {
  const [updateInfo, setUpdateInfo] = useState<AppUpdateInfo>(initialUpdateState);
  const pendingUpdateRef = useRef<Update | null>(null);

  const clearPendingUpdate = useCallback(async () => {
    if (!pendingUpdateRef.current) {
      return;
    }

    const update = pendingUpdateRef.current;
    pendingUpdateRef.current = null;
    try {
      await update.close();
    } catch {
      // Ignore resource cleanup failures.
    }
  }, []);

  const checkForUpdates = useCallback(
    async (source: AppUpdateCheckSource = "manual") => {
      setUpdateInfo((prev) => ({
        ...prev,
        status: "checking",
        error: null,
        source,
        downloaded_bytes: 0,
        content_length: null,
        download_percent: null,
      }));

      const currentVersion = normalizeVersion(await getVersion());
      if (!currentVersion) {
        setUpdateInfo((prev) => ({
          ...prev,
          status: "error",
          error: "Cannot determine current app version",
          checked_at: Date.now(),
          source,
          current_version: null,
        }));
        return null;
      }

      try {
        const update = await check();
        await clearPendingUpdate();

        if (!update) {
          pendingUpdateRef.current = null;
          setUpdateInfo({
            ...initialUpdateState,
            status: "up_to_date",
            current_version: currentVersion,
            checked_at: Date.now(),
            source,
          });
          return null;
        }

        pendingUpdateRef.current = update;
        const latestVersion = normalizeVersion(update.version) ?? update.version;
        const releaseUrl =
          typeof update.rawJson?.url === "string"
            ? update.rawJson.url
            : LATEST_RELEASE_PAGE;

        setUpdateInfo({
          ...initialUpdateState,
          status: "available",
          current_version: currentVersion,
          latest_version: latestVersion,
          release_name: `Codex Quota Monitor v${latestVersion}`,
          release_url: releaseUrl,
          published_at: update.date ?? null,
          body: update.body ?? null,
          checked_at: Date.now(),
          source,
          can_download_and_install: true,
        });
        return update;
      } catch (primaryError) {
        await clearPendingUpdate();

        try {
          const release = await fetchLatestGithubRelease();
          const latestVersion = normalizeVersion(release.tag_name);

          if (!latestVersion) {
            throw new Error("Latest release tag is missing or invalid");
          }

          const isUpdateAvailable = compareVersions(currentVersion, latestVersion) < 0;

          setUpdateInfo({
            ...initialUpdateState,
            status: isUpdateAvailable ? "available" : "up_to_date",
            current_version: currentVersion,
            latest_version: latestVersion,
            release_name: release.name ?? release.tag_name ?? null,
            release_url: release.html_url ?? LATEST_RELEASE_PAGE,
            published_at: release.published_at ?? null,
            body: release.body ?? null,
            checked_at: Date.now(),
            source,
            can_download_and_install: false,
            error: isUpdateAvailable
              ? "Updater artifacts are not available for this release yet. Open the release page to install manually."
              : null,
          });
          return null;
        } catch (fallbackError) {
          const fallbackMessage =
            fallbackError instanceof Error ? fallbackError.message : String(fallbackError);
          const primaryMessage =
            primaryError instanceof Error ? primaryError.message : String(primaryError);

          setUpdateInfo({
            ...initialUpdateState,
            status: "error",
            current_version: currentVersion,
            error: `${primaryMessage}. ${fallbackMessage}`,
            checked_at: Date.now(),
            source,
            release_url: LATEST_RELEASE_PAGE,
          });
          return null;
        }
      }
    },
    [clearPendingUpdate]
  );

  const installUpdate = useCallback(async () => {
    const update = pendingUpdateRef.current;
    if (!update) {
      throw new Error("No pending updater package is available");
    }
    pendingUpdateRef.current = null;

    let downloadedBytes = 0;
    setUpdateInfo((prev) => ({
      ...prev,
      status: "downloading",
      error: null,
      downloaded_bytes: 0,
      content_length: null,
      download_percent: null,
    }));

    await update.downloadAndInstall((event: DownloadEvent) => {
      if (event.event === "Started") {
        downloadedBytes = 0;
        setUpdateInfo((prev) => ({
          ...prev,
          status: "downloading",
          downloaded_bytes: 0,
          content_length: event.data.contentLength ?? null,
          download_percent: computeProgressPercent(0, event.data.contentLength ?? null),
        }));
        return;
      }

      if (event.event === "Progress") {
        downloadedBytes += event.data.chunkLength;
        setUpdateInfo((prev) => ({
          ...prev,
          status: "downloading",
          downloaded_bytes: downloadedBytes,
          download_percent: computeProgressPercent(downloadedBytes, prev.content_length),
        }));
        return;
      }

      if (event.event === "Finished") {
        setUpdateInfo((prev) => ({
          ...prev,
          status: "installing",
          download_percent: prev.content_length ? 100 : prev.download_percent,
        }));
      }
    });

    setUpdateInfo((prev) => ({
      ...prev,
      status: "relaunching",
    }));

    try {
      await update.close();
    } catch {
      // Ignore resource cleanup failures.
    }

    await relaunch();
  }, []);

  useEffect(() => {
    if (import.meta.env.DEV) {
      return;
    }

    void checkForUpdates("auto");
  }, [checkForUpdates]);

  useEffect(() => {
    return () => {
      void clearPendingUpdate();
    };
  }, [clearPendingUpdate]);

  return {
    updateInfo,
    isCheckingForUpdates: updateInfo.status === "checking",
    isInstallingUpdate:
      updateInfo.status === "downloading" ||
      updateInfo.status === "installing" ||
      updateInfo.status === "relaunching",
    checkForUpdates,
    installUpdate,
  };
}
