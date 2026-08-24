import { execFileSync, spawnSync } from "node:child_process";
import {
  accessSync,
  constants,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import {
  test,
  expect,
  _electron as electron,
  type ElectronApplication,
  type Page,
} from "@playwright/test";
import "./api-types";
import { createProjectOperation } from "./project-helper";

const require = createRequire(import.meta.url);
const electronExecutable = require("electron") as string;
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const hostBinary = resolve(
  studioRoot,
  "../target/debug/double-love-desktop-host",
);
const mainEntry = resolve(studioRoot, "out/main/index.js");

let temporaryRoot: string;

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK);
  accessSync(mainEntry, constants.R_OK);
}

async function launch(
  userData: string,
): Promise<{ app: ElectronApplication; page: Page }> {
  const app = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      "--double-love-e2e",
      "--double-love-e2e-transcribe-mock",
      `--double-love-e2e-user-data=${userData}`,
    ],
    env: { ...process.env, ELECTRON_DISABLE_SECURITY_WARNINGS: "true" },
  });
  const page = await app.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  await expect
    .poll(async () => {
      const response = await page.evaluate(() =>
        window.doubleLove.hostHealth(),
      );
      return (response as { status?: string }).status;
    })
    .toBe("ok");
  return { app, page };
}

async function invoke<T>(
  page: Page,
  name: string,
  payload?: unknown,
): Promise<T> {
  const response = (await page.evaluate(
    async ({ command, commandPayload }) =>
      window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  )) as {
    status: string;
    result?: { type: string; data?: T };
    error?: { code: string };
  };
  if (response.status !== "ok" || response.result?.type !== "invoke")
    return response as unknown as T;
  return response.result.data as T;
}

function hostPidFor(appPid: number): number {
  // Only the direct host child of THIS Electron main process may be killed.
  const listing = spawnSync("ps", ["-eo", "pid,ppid,comm"], {
    encoding: "utf8",
  });
  const line = listing.stdout
    .split("\n")
    .find(
      (entry) =>
        entry.includes(hostBinary) &&
        Number(entry.trim().split(/\s+/)[1]) === appPid,
    );
  if (!line) throw new Error("host child not found under this app process");
  return Number(line.trim().split(/\s+/)[0]);
}

test.beforeAll(() => {
  requireBuildArtifacts();
  temporaryRoot = mkdtempSync(
    join(tmpdir(), "double-love-electron-crash-e2e-"),
  );
  // Seed installed model state before launch: the host's model manager reads it at startup.
  mkdirSyncForModels(join(temporaryRoot, "user-data"));
});

test.afterAll(() => {
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true });
});

test("a killed host rejects calls, marks the crash, and never replays writes", async () => {
  const userData = join(temporaryRoot, "user-data");
  const projectRoot = join(temporaryRoot, "project");
  const mediaPath = join(temporaryRoot, "crash-fixture.mp4");
  execFileSync(process.env.DOUBLELOVE_FFMPEG || "ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-f",
    "lavfi",
    "-i",
    "color=c=black:s=320x180:r=25:d=61",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=440:sample_rate=48000:duration=61",
    "-c:v",
    "libx264",
    "-pix_fmt",
    "yuv420p",
    "-c:a",
    "aac",
    "-shortest",
    mediaPath,
  ]);

  const first = await launch(userData);
  const created = await createProjectOperation<{ status: string }>(
    first.page,
    projectRoot,
  );
  expect(created.status).toBe("success");

  const mediaGrant = await first.page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickMediaFile({ e2ePath }),
    mediaPath,
  );
  const imported = await invoke<{ status: string; data?: { id: string } }>(
    first.page,
    "import_media",
    { grantToken: mediaGrant?.token },
  );
  expect(imported.status).toBe("success");
  const assetId = imported.data?.id as string;

  const started = await invoke<{ status: string; data?: { task_id: string } }>(
    first.page,
    "transcribe_start",
    { assetId, model: "qwen3-asr-0.6b-4bit", language: "auto" },
  );
  expect(started.status).toBe("success");

  // Kill ONLY this app's host child mid-transcription (a write task in flight).
  const appPid = first.app.process().pid;
  if (!appPid) throw new Error("app pid unavailable");
  process.kill(hostPidFor(appPid), "SIGKILL");

  await expect
    .poll(
      async () => {
        const response = await first.page.evaluate(() =>
          window.doubleLove
            .invoke("assets_list", {})
            .catch((error: unknown) => ({
              ipcError: error instanceof Error ? error.message : "unknown",
            })),
        );
        return response as Record<string, unknown>;
      },
      { timeout: 10_000 },
    )
    .toMatchObject({
      status: "error",
      error: { code: "HOST_UNAVAILABLE" },
    });

  // Crash marker exists and contains only ts/exitCode/signal — no paths or payloads.
  const crashMarker = join(userData, "logs", "host-crash.json");
  await expect
    .poll(() => existsSync(crashMarker), { timeout: 10_000 })
    .toBe(true);
  const marker = JSON.parse(readFileSync(crashMarker, "utf8")) as Record<
    string,
    unknown
  >;
  expect(Object.keys(marker).sort()).toEqual(["exitCode", "signal", "ts"]);
  expect(JSON.stringify(marker)).not.toContain("/");
  expect(JSON.stringify(marker)).not.toContain(temporaryRoot);

  await first.app.close();

  // Relaunch with the same userData: fresh host, project reopens, and the interrupted
  // transcription was NOT replayed — no words were committed by the crashed task.
  const second = await launch(userData);
  const reopenGrant = await second.page.evaluate(
    async (e2ePath) =>
      window.doubleLove.dialogs.pickDirectory({
        title: "crash project",
        kind: "project-open",
        e2ePath,
      }),
    projectRoot,
  );
  const reopened = await invoke<{ status: string }>(
    second.page,
    "project_open",
    { grantToken: reopenGrant?.token },
  );
  expect(reopened.status).toBe("success");
  const transcript = await invoke<{
    status: string;
    data?: { words: unknown[] };
  }>(second.page, "transcript_get", { assetId });
  expect(transcript.status).toBe("success");
  expect(transcript.data?.words).toEqual([]);

  await second.app.close();
});

function mkdirSyncForModels(userData: string): void {
  const modelRoot = join(userData, "models");
  const installations = {
    schema_version: 1,
    installations: {
      "qwen3-asr-0.6b-4bit": {
        model_id: "qwen3-asr-0.6b-4bit",
        revision: "70ccd0ba0c24b0c78efc313ce81c1c78c64a3dd7",
        state: "installed",
        bytes_downloaded: 0,
        bytes_total: 0,
        staging_id: null,
        last_error_code: null,
        last_error_message: null,
        updated_at: "2026-01-01T00:00:00Z",
      },
      "qwen3-forced-aligner-0.6b-8bit": {
        model_id: "qwen3-forced-aligner-0.6b-8bit",
        revision: "998b617c695f61865d444c62051fe51030acef6f",
        state: "installed",
        bytes_downloaded: 0,
        bytes_total: 0,
        staging_id: null,
        last_error_code: null,
        last_error_message: null,
        updated_at: "2026-01-01T00:00:00Z",
      },
    },
  };
  mkdirSync(modelRoot, { recursive: true });
  writeFileSync(
    join(modelRoot, "installations.json"),
    `${JSON.stringify(installations, null, 2)}\n`,
  );
}
