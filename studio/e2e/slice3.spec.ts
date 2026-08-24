import { execFileSync } from "node:child_process";
import {
  accessSync,
  constants,
  mkdtempSync,
  readFileSync,
  realpathSync,
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

interface MediaAssetSummary {
  id: string;
  display_name: string;
  status: string;
}

let electronApp: ElectronApplication;
let page: Page;
let temporaryRoot: string;
let projectRoot: string;
let mediaPath: string;
let probeFailureMediaPath: string;

function requireBuildArtifacts(): void {
  accessSync(hostBinary, constants.X_OK);
  accessSync(mainEntry, constants.R_OK);
}

function generateMedia(path: string): void {
  execFileSync(process.env.DOUBLELOVE_FFMPEG || "ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
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
    path,
  ]);
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

async function mediaGrant(path: string): Promise<string> {
  const grant = await page.evaluate(
    async (e2ePath) => window.doubleLove.dialogs.pickMediaFile({ e2ePath }),
    path,
  );
  expect(grant).toEqual({ token: expect.any(String) });
  return grant?.token as string;
}

test.beforeAll(async () => {
  requireBuildArtifacts();
  temporaryRoot = mkdtempSync(
    join(tmpdir(), "double-love-electron-slice3-e2e-"),
  );
  projectRoot = join(temporaryRoot, "project");
  mediaPath = join(temporaryRoot, "synthetic.mp4");
  probeFailureMediaPath = join(temporaryRoot, "synthetic-ffprobe-failure.mp4");
  generateMedia(mediaPath);
  writeFileSync(probeFailureMediaPath, "synthetic invalid media");

  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      "--double-love-e2e",
      `--double-love-e2e-user-data=${join(temporaryRoot, "user-data")}`,
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
});

test.afterAll(async () => {
  if (electronApp) await electronApp.close();
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true });
});

test("imports, lists, and serves a real project asset through dl-media", async () => {
  const created = await createProjectOperation<
    InvokeOperation<{ project_id: string }>
  >(page, projectRoot);
  expect(created.status).toBe("success");

  const imported = await invoke<MediaAssetSummary>("import_media", {
    grantToken: await mediaGrant(mediaPath),
  });
  expect(imported).toMatchObject({
    status: "success",
    data: {
      id: expect.any(String),
      display_name: "synthetic.mp4",
      status: "imported",
      prepared_available: false,
    },
  });
  expect(JSON.stringify(imported)).not.toContain(mediaPath);
  const assetId = imported.data?.id as string;

  const listed = await invoke<MediaAssetSummary[]>("assets_list");
  expect(listed.status).toBe("success");
  expect(listed.data).toEqual([
    expect.objectContaining({
      id: assetId,
      status: "imported",
      prepared_available: false,
    }),
  ]);
  expect(Object.keys(listed.data?.[0] ?? {}).sort()).toEqual([
    "audio_channels",
    "audio_sample_rate",
    "display_name",
    "duration_samples",
    "height",
    "id",
    "prepared_available",
    "rate",
    "status",
    "width",
  ]);
  expect(JSON.stringify(listed)).not.toContain(mediaPath);
  expect(JSON.stringify(listed)).not.toContain(realpathSync(mediaPath));
  expect(JSON.stringify(listed)).not.toContain(projectRoot);
  expect(JSON.stringify(listed)).not.toContain(realpathSync(projectRoot));

  const duplicate = await invoke<MediaAssetSummary>("import_media", {
    grantToken: await mediaGrant(mediaPath),
  });
  expect(duplicate).toMatchObject({ status: "success", data: { id: assetId } });
  expect(duplicate.diagnostics).toEqual(
    expect.arrayContaining([
      expect.objectContaining({ code: "MEDIA_ALREADY_IMPORTED" }),
    ]),
  );

  const expected = readFileSync(mediaPath);
  const full = await electronApp.evaluate(async ({ net }, id) => {
    const response = await net.fetch(
      `dl-media://asset/${encodeURIComponent(id)}`,
    );
    return {
      status: response.status,
      acceptRanges: response.headers.get("accept-ranges"),
      contentLength: response.headers.get("content-length"),
      body: Array.from(new Uint8Array(await response.arrayBuffer())),
    };
  }, assetId);
  expect(full).toEqual({
    status: 200,
    acceptRanges: "bytes",
    contentLength: String(expected.length),
    body: Array.from(expected),
  });

  const range = await electronApp.evaluate(async ({ net }, id) => {
    const response = await net.fetch(
      `dl-media://asset/${encodeURIComponent(id)}`,
      {
        headers: { Range: "bytes=0-31" },
      },
    );
    return {
      status: response.status,
      contentRange: response.headers.get("content-range"),
      contentLength: response.headers.get("content-length"),
      body: Array.from(new Uint8Array(await response.arrayBuffer())),
    };
  }, assetId);
  expect(range).toEqual({
    status: 206,
    contentRange: `bytes 0-31/${expected.length}`,
    contentLength: "32",
    body: Array.from(expected.subarray(0, 32)),
  });

  const rejectedUrls = await electronApp.evaluate(
    async ({ net }, sourcePath) => {
      const urls = [
        `dl-media://asset/${crypto.randomUUID()}`,
        `dl-media://asset/${encodeURIComponent(sourcePath)}`,
        `dl-media://file/${encodeURIComponent(sourcePath)}`,
      ];
      return Promise.all(
        urls.map(async (url) => (await net.fetch(url)).status),
      );
    },
    mediaPath,
  );
  expect(rejectedUrls).toEqual([404, 404, 404]);

  const rendererResolve = (await page.evaluate(() =>
    window.doubleLove.invoke("resolve_media_asset", { asset_id: "probe" }),
  )) as InvokeEnvelope<unknown>;
  expect(rendererResolve).toMatchObject({
    status: "error",
    error: { code: "IPC_FORBIDDEN" },
  });

  const missingPath = join(temporaryRoot, "missing.mp4");
  const missing = await invoke<MediaAssetSummary>("import_media", {
    grantToken: await mediaGrant(missingPath),
  });
  expect(missing.status).toBe("failed");
  expect(missing.diagnostics[0]?.code).toBe("MEDIA_FILE_MISSING");
  expect(JSON.stringify(missing)).not.toContain(missingPath);
  expect(JSON.stringify(missing)).not.toContain(projectRoot);
  expect(JSON.stringify(missing)).not.toContain(realpathSync(projectRoot));
  expect(JSON.stringify(missing)).toContain("<SELECTED_MEDIA>");

  const probeFailure = await invoke<MediaAssetSummary>("import_media", {
    grantToken: await mediaGrant(probeFailureMediaPath),
  });
  expect(probeFailure.status).toBe("failed");
  expect(probeFailure.diagnostics[0]?.code).toBe("MEDIA_PROBE_FAILED");
  expect(JSON.stringify(probeFailure)).not.toContain(probeFailureMediaPath);
  expect(JSON.stringify(probeFailure)).not.toContain(
    realpathSync(probeFailureMediaPath),
  );
  expect(JSON.stringify(probeFailure)).not.toContain(projectRoot);
  expect(JSON.stringify(probeFailure)).not.toContain(realpathSync(projectRoot));
  expect(JSON.stringify(probeFailure)).toContain("<SELECTED_MEDIA>");

  expect((await invoke<MediaAssetSummary[]>("assets_list")).data).toHaveLength(
    1,
  );
});
