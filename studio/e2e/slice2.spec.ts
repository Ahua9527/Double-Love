import { accessSync, constants, mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
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
const cliBinary = resolve(studioRoot, "../target/debug/double-love");
const mainEntry = resolve(studioRoot, "out/main/index.js");

interface InvokeOperation<T> {
  status: "success" | "partial" | "failed" | "cancelled";
  revision: number | null;
  data: T | null;
  diagnostics: Array<{ code: string }>;
}

interface InvokeEnvelope<T> {
  status: "ok" | "error";
  result?: { type: string; data?: InvokeOperation<T> };
  error?: { code: string };
}

interface ProjectSummary {
  project_id: string;
  root: string;
  database: string;
  revision: number;
}

let electronApp: ElectronApplication;
let page: Page;
let temporaryRoot: string;
let userData: string;
let projectRoot: string;

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK);
  accessSync(cliBinary, constants.X_OK);
  accessSync(mainEntry, constants.R_OK);
}

async function launch(): Promise<void> {
  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      "--double-love-e2e",
      `--double-love-e2e-user-data=${userData}`,
    ],
    env: {
      ...process.env,
      ELECTRON_DISABLE_SECURITY_WARNINGS: "true",
    },
  });
  page = await electronApp.firstWindow();
  await page.waitForLoadState("domcontentloaded");
  await expect
    .poll(async () => {
      const response = await page.evaluate(() =>
        window.doubleLove.hostHealth(),
      );
      return (response as { status?: string }).status;
    })
    .toBe("ok");
}

async function invoke<T>(
  name: string,
  payload?: unknown,
): Promise<InvokeOperation<T>> {
  const response = (await page.evaluate(
    async ({ command, commandPayload }) =>
      window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  )) as InvokeEnvelope<T>;
  expect(response.status).toBe("ok");
  expect(response.result?.type).toBe("invoke");
  return response.result?.data as InvokeOperation<T>;
}

async function directoryGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) =>
      window.doubleLove.dialogs.pickDirectory({
        title: "选择项目目录",
        kind: "project-open",
        e2ePath,
      }),
    path,
  );
  expect(grant?.token).toBeTruthy();
  return grant?.token as string;
}

async function createProject(
  path: string,
): Promise<InvokeOperation<ProjectSummary>> {
  return createProjectOperation(page, path);
}

async function openProject(
  path: string,
): Promise<InvokeOperation<ProjectSummary>> {
  return invoke<ProjectSummary>("project_open", {
    grantToken: await directoryGrant(path),
  });
}

function cliOperation(
  args: string[],
): InvokeOperation<Record<string, unknown>> {
  return JSON.parse(
    execFileSync(cliBinary, ["--json", "--project", projectRoot, ...args], {
      encoding: "utf8",
    }),
  ) as InvokeOperation<Record<string, unknown>>;
}

function addMainTrackClipWithCli(): void {
  const media = join(temporaryRoot, "external-cli.mp4");
  execFileSync(
    "ffmpeg",
    [
      "-y",
      "-f",
      "lavfi",
      "-i",
      "color=c=black:s=320x180:r=25:d=1",
      "-f",
      "lavfi",
      "-i",
      "sine=frequency=440:sample_rate=48000:duration=1",
      "-c:v",
      "libx264",
      "-pix_fmt",
      "yuv420p",
      "-c:a",
      "aac",
      "-shortest",
      media,
    ],
    { stdio: "ignore" },
  );
  const imported = cliOperation(["import-media", "--file", media]);
  expect(imported.status).toBe("success");
  const assetId = imported.data?.id;
  expect(typeof assetId).toBe("string");
  const appended = cliOperation([
    "main-track-add",
    "--asset",
    assetId as string,
  ]);
  expect(appended.status).toBe("success");
  expect(appended.revision).toBe(6);
}

test.beforeAll(async () => {
  requireBuildArtifacts();
  temporaryRoot = mkdtempSync(
    join(tmpdir(), "double-love-electron-slice2-e2e-"),
  );
  userData = join(temporaryRoot, "user-data");
  projectRoot = join(temporaryRoot, "project");
  await launch();
});

test.afterAll(async () => {
  if (electronApp) await electronApp.close();
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true });
});

test("creates, reopens, navigates history, restores, and persists project identity", async () => {
  const closed = await invoke<number>("project_revision");
  expect(closed.status).toBe("failed");
  expect(closed.diagnostics[0]?.code).toBe("PROJECT_NOT_OPEN");

  const created = await createProject(projectRoot);
  expect(created.status).toBe("success");
  expect(created.data?.revision).toBe(1);
  const projectId = created.data?.project_id;
  expect(projectId).toBeTruthy();

  const initialHistory =
    await invoke<
      Array<{ revision: number; operation: string; restorable: boolean }>
    >("project_history");
  expect(initialHistory.data?.[0]).toMatchObject({
    revision: 1,
    operation: "subtitle_style_set",
    restorable: true,
  });

  const persistedDefaultStyle =
    await invoke<Record<string, unknown>>("subtitle_style_get");
  expect(persistedDefaultStyle).toMatchObject({
    status: "success",
    data: {
      font_family: "PingFang SC",
      font_size: 52,
      cjk_spacing: true,
    },
  });
  const changedStyle = {
    ...persistedDefaultStyle.data,
    font_size: 54,
  };
  const styleChanged = await invoke<Record<string, unknown>>(
    "subtitle_style_set",
    { style: changedStyle },
  );
  expect(styleChanged).toMatchObject({
    status: "success",
    revision: 2,
    data: {
      font_family: "PingFang SC",
      font_size: 54,
      cjk_spacing: true,
    },
  });
  const styleHistory = await invoke<
    Array<{ revision: number; operation: string; restorable: boolean }>
  >("project_history", { limit: 1 });
  expect(styleHistory.data).toEqual([
    expect.objectContaining({
      revision: 2,
      operation: "subtitle_style_set",
      restorable: true,
    }),
  ]);

  const recent = await invoke<
    Array<{
      project_id: string;
      root: string;
      exists: boolean;
      created_at: string | null;
      modified_at: string | null;
      canvas: { width: number; height: number } | null;
      output_rate: string | null;
    }>
  >("recent_projects_list");
  expect(recent.status).toBe("success");
  expect(recent.data).toHaveLength(1);
  expect(recent.data?.[0]).toMatchObject({
    project_id: projectId,
    exists: true,
    created_at: expect.any(String),
    modified_at: expect.any(String),
    canvas: expect.objectContaining({ width: 1920, height: 1080 }),
    output_rate: null,
  });
  expect(
    await invoke<{ project_library_view: string }>("preferences_update", {
      patch: { project_library_view: "list" },
    }),
  ).toMatchObject({
    status: "success",
    data: { project_library_view: "list" },
  });

  const reopened = await openProject(projectRoot);
  expect(reopened.status).toBe("success");
  expect(reopened.data?.project_id).toBe(projectId);
  expect(reopened.data?.revision).toBe(2);

  const changedCanvas = {
    width: 1280,
    height: 720,
    background: "#112233",
    fit: "cover",
    position_x: 5,
    position_y: -2,
    scale: 1.2,
    rotation_degrees: 3,
    opacity: 0.9,
  };
  const changed = await invoke<typeof changedCanvas>("canvas_set", {
    canvas: changedCanvas,
  });
  expect(changed).toMatchObject({
    status: "success",
    revision: 3,
    data: changedCanvas,
  });
  expect((await invoke<number>("project_revision")).data).toBe(3);

  const history = await invoke<
    Array<{ revision: number; operation: string; restorable: boolean }>
  >("project_history", { limit: 10 });
  expect(history.data).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        revision: 3,
        operation: "canvas_set",
        restorable: true,
      }),
    ]),
  );

  const undo = await invoke<null>("edit_undo");
  expect(undo).toMatchObject({ status: "success", revision: 4 });
  expect((await invoke<{ width: number }>("canvas_get")).data?.width).toBe(
    1920,
  );

  const redo = await invoke<null>("edit_redo");
  expect(redo).toMatchObject({ status: "success", revision: 5 });
  expect((await invoke<{ width: number }>("canvas_get")).data?.width).toBe(
    1280,
  );

  addMainTrackClipWithCli();
  expect((await invoke<number>("project_revision")).data).toBe(6);
  const historyAfterCli = await invoke<
    Array<{ revision: number; operation: string; restorable: boolean }>
  >("project_history", { limit: 10 });
  expect(historyAfterCli.data).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        revision: 3,
        operation: "canvas_set",
        restorable: true,
      }),
      expect.objectContaining({
        revision: 6,
        operation: "main_track_append",
        restorable: true,
      }),
    ]),
  );

  const restored = await invoke<{
    restored_revision: number;
    revision: number;
  }>("project_restore_revision", { revision: 1 });
  expect(restored).toMatchObject({
    status: "success",
    revision: 7,
    data: { restored_revision: 1, revision: 7 },
  });
  expect((await invoke<number>("project_revision")).data).toBe(7);

  const invalidRoot = join(temporaryRoot, "not-a-project");
  mkdirSync(invalidRoot);
  const invalid = await openProject(invalidRoot);
  expect(invalid.status).toBe("failed");
  expect(invalid.diagnostics[0]?.code).toBe("PROJECT_OPEN_FAILED");
  expect((await invoke<number>("project_revision")).data).toBe(7);

  await electronApp.close();
  await launch();
  const openedAfterRestart = await openProject(projectRoot);
  expect(openedAfterRestart.status).toBe("success");
  expect(openedAfterRestart.data?.project_id).toBe(projectId);
  expect(openedAfterRestart.data?.revision).toBe(7);
  expect(
    (await invoke<{ project_library_view: string }>("preferences_get")).data
      ?.project_library_view,
  ).toBe("list");
});
