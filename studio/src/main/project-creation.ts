import { existsSync } from "node:fs";
import { isAbsolute, join } from "node:path";

export type ProjectCreationErrorCode =
  | "PROJECT_NAME_REQUIRED"
  | "PROJECT_NAME_INVALID"
  | "PROJECT_PARENT_INVALID"
  | "PROJECT_ALREADY_EXISTS";

type ProjectNameResult =
  | { ok: true; name: string }
  | { ok: false; code: ProjectCreationErrorCode; message: string };

export type ProjectTargetResult =
  | { ok: true; name: string; parent: string; target: string }
  | { ok: false; code: ProjectCreationErrorCode; message: string };

export function validateProjectName(value: unknown): ProjectNameResult {
  if (typeof value !== "string" || value.trim().length === 0) {
    return {
      ok: false,
      code: "PROJECT_NAME_REQUIRED",
      message: "请输入项目名称。",
    };
  }
  const name = value.trim();
  if (name === "." || name === ".." || /[\\/\0]/u.test(name)) {
    return {
      ok: false,
      code: "PROJECT_NAME_INVALID",
      message: "项目名称不能包含路径分隔符。",
    };
  }
  return { ok: true, name };
}

export function resolveProjectTarget(
  options: { name: unknown; moviesDirectory: string; customParent?: string },
  pathExists: (path: string) => boolean = existsSync,
): ProjectTargetResult {
  const validated = validateProjectName(options.name);
  if (!validated.ok) return validated;
  const parent =
    options.customParent ??
    join(options.moviesDirectory, "Double Love Projects");
  if (!isAbsolute(parent)) {
    return {
      ok: false,
      code: "PROJECT_PARENT_INVALID",
      message: "项目保存位置无效。",
    };
  }
  const target = join(parent, validated.name);
  if (pathExists(target)) {
    return {
      ok: false,
      code: "PROJECT_ALREADY_EXISTS",
      message: "这个位置已经有同名文件夹，请换一个名称。",
    };
  }
  return { ok: true, name: validated.name, parent, target };
}
