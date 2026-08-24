import { basename, dirname } from "node:path";
import type { Page } from "@playwright/test";
import "./api-types";

interface InvokeEnvelope {
  status: "ok" | "error";
  result?: { type: string; data?: unknown };
  error?: { code: string; message: string };
}

export async function createProjectOperation<T>(
  page: Page,
  projectRoot: string,
): Promise<T> {
  const response = (await page.evaluate(
    async ({ name, parent }) => {
      const grant = await window.doubleLove.dialogs.pickDirectory({
        title: "Create synthetic project",
        kind: "project-parent",
        e2ePath: parent,
      });
      if (!grant) throw new Error("Expected project parent grant");
      return window.doubleLove.createProject({
        name,
        parentGrantToken: grant.token,
      });
    },
    { name: basename(projectRoot), parent: dirname(projectRoot) },
  )) as InvokeEnvelope;

  if (
    response.status !== "ok" ||
    response.result?.type !== "invoke" ||
    response.result.data === undefined
  ) {
    throw new Error(
      response.error?.message ??
        "Project creation did not return an operation result",
    );
  }
  return response.result.data as T;
}
