import {
  accessSync,
  constants,
  mkdirSync,
  mkdtempSync,
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

const require = createRequire(import.meta.url);
const electronExecutable = require("electron") as string;
const studioRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const hostBinary = resolve(
  studioRoot,
  "../target/debug/double-love-desktop-host",
);
const mainEntry = resolve(studioRoot, "out/main/index.js");

interface CatalogEntry {
  descriptor: { id: string };
  installation: { state: string; bytes_downloaded: number };
}

interface InvokeEnvelope<T> {
  status: "ok" | "error";
  result?: {
    type: string;
    data?: {
      status: "success" | "failed";
      data?: T | null;
    };
  };
}

let electronApp: ElectronApplication;
let page: Page;
let temporaryRoot: string;
let userDataPath: string;

function seedPausedQwen(userData: string): void {
  const modelRoot = join(userData, "models");
  mkdirSync(modelRoot, { recursive: true });
  const installation = (
    modelId: string,
    revision: string,
    state: string,
    bytesDownloaded: number,
    bytesTotal: number,
    stagingId: string | null,
  ) => ({
    model_id: modelId,
    revision,
    state,
    bytes_downloaded: bytesDownloaded,
    bytes_total: bytesTotal,
    staging_id: stagingId,
    last_error_code: null,
    last_error_message: null,
    updated_at: "2026-01-01T00:00:00Z",
  });
  writeFileSync(
    join(modelRoot, "installations.json"),
    JSON.stringify(
      {
        schema_version: 1,
        installations: {
          "qwen3-asr-0.6b-4bit": installation(
            "qwen3-asr-0.6b-4bit",
            "70ccd0ba0c24b0c78efc313ce81c1c78c64a3dd7",
            "paused",
            100,
            708236945,
            "cancel-stage",
          ),
          "qwen3-forced-aligner-0.6b-8bit": installation(
            "qwen3-forced-aligner-0.6b-8bit",
            "998b617c695f61865d444c62051fe51030acef6f",
            "installed",
            1271924386,
            1271924386,
            null,
          ),
        },
      },
      null,
      2,
    ),
  );
}

async function invoke<T>(
  name: string,
  payload?: unknown,
): Promise<T> {
  const response = (await page.evaluate(
    async ({ command, commandPayload }) =>
      window.doubleLove.invoke(command, commandPayload),
    { command: name, commandPayload: payload },
  )) as InvokeEnvelope<T>;
  expect(response.status).toBe("ok");
  expect(response.result?.type).toBe("invoke");
  expect(response.result?.data?.status).toBe("success");
  return response.result?.data?.data as T;
}

test.beforeAll(async () => {
  accessSync(hostBinary, constants.X_OK);
  accessSync(mainEntry, constants.R_OK);
  temporaryRoot = mkdtempSync(join(tmpdir(), "double-love-electron-model-cancel-"));
  userDataPath = join(temporaryRoot, "user-data");
  seedPausedQwen(userDataPath);
  electronApp = await electron.launch({
    executablePath: electronExecutable,
    args: [
      studioRoot,
      "--double-love-e2e",
      `--double-love-e2e-user-data=${userDataPath}`,
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
      const response = await page.evaluate(() => window.doubleLove.hostHealth());
      return (response as { status?: string }).status;
    })
    .toBe("ok");
  await page.evaluate(() =>
    window.doubleLove.invoke("onboarding_complete", { step: 3 }),
  );
  await page.reload();
  await page.waitForLoadState("domcontentloaded");
});

test.afterAll(async () => {
  if (electronApp) await electronApp.close();
  if (temporaryRoot) rmSync(temporaryRoot, { recursive: true, force: true });
});

test("clears a paused model task and broadcasts the final state", async () => {
  const states: Array<{ model_id: string; state: string }> = [];
  await page.evaluate(() => {
    const captured = window as unknown as {
      modelCancelStates?: Array<{ model_id: string; state: string }>;
    };
    captured.modelCancelStates = [];
    window.doubleLove.onEvent("dl://model-state", (payload) => {
      captured.modelCancelStates?.push(
        payload as { model_id: string; state: string },
      );
    });
  });
  await page.getByRole("button", { name: /后台任务/ }).click();
  await expect(page.getByRole("heading", { name: "需要处理" })).toBeVisible();
  expect(await page.getByRole("progressbar").count()).toBe(0);
  await page.getByRole("button", { name: "清除任务" }).click();
  await expect(page.getByText("当前没有后台任务")).toBeVisible();

  const catalog = await invoke<CatalogEntry[]>("model_catalog");
  const qwen = catalog.find(
    (entry) => entry.descriptor.id === "qwen3-asr-0.6b-4bit",
  );
  expect(qwen?.installation.state).toBe("not_installed");
  expect(qwen?.installation.bytes_downloaded).toBe(100);

  const observed = await page.evaluate(() => {
    const captured = window as unknown as {
      modelCancelStates?: Array<{ model_id: string; state: string }>;
    };
    return captured.modelCancelStates ?? [];
  });
  states.push(...observed);
  expect(states).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        model_id: "qwen3-asr-0.6b-4bit",
        state: "not_installed",
      }),
    ]),
  );
});
