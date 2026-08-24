// @vitest-environment node

import { describe, expect, it } from "vitest";
import { resolveProjectTarget, validateProjectName } from "./project-creation";

describe("secure project creation target", () => {
  it("trims Chinese project names and uses the Movies default parent", () => {
    expect(
      resolveProjectTarget(
        {
          name: "  春日采访  ",
          moviesDirectory: "/Users/editor/Movies",
        },
        () => false,
      ),
    ).toEqual({
      ok: true,
      name: "春日采访",
      parent: "/Users/editor/Movies/Double Love Projects",
      target: "/Users/editor/Movies/Double Love Projects/春日采访",
    });
  });

  it.each(["", "   ", "../采访", "采访/第一天", "采访\\第一天", ".", ".."])(
    "rejects an unsafe project name: %j",
    (name) => expect(validateProjectName(name)).toMatchObject({ ok: false }),
  );

  it("supports the isolated E2E parent override used outside packaged builds", () => {
    expect(
      resolveProjectTarget(
        {
          name: "外接盘项目",
          moviesDirectory: "/Users/editor/Movies",
          customParent: "/Volumes/Work",
        },
        () => false,
      ),
    ).toMatchObject({
      ok: true,
      parent: "/Volumes/Work",
      target: "/Volumes/Work/外接盘项目",
    });
  });

  it("reports an inline-safe conflict instead of overwriting", () => {
    expect(
      resolveProjectTarget(
        {
          name: "已存在",
          moviesDirectory: "/Users/editor/Movies",
        },
        () => true,
      ),
    ).toEqual({
      ok: false,
      code: "PROJECT_ALREADY_EXISTS",
      message: "这个位置已经有同名文件夹，请换一个名称。",
    });
  });
});
