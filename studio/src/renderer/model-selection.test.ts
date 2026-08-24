import { describe, expect, it } from "vitest";
import type { ModelDescriptor } from "./platform/desktop";
import { selectAsrModel } from "./model-selection";

function model(id: string, state: ModelDescriptor["state"]): ModelDescriptor {
  return {
    id, label: id, kind: "asr", ui_role: "primary", download_source: "modelscope", revision: "test",
    size_bytes: 1, memory_bytes: 1, license: "test", dependencies: [], state,
  };
}

function legacyModel(id: string, state: ModelDescriptor["state"]): ModelDescriptor {
  return { ...model(id, state), ui_role: "legacy" };
}

describe("automatic ASR selection", () => {
  it("uses the installed recommended model first", () => {
    expect(selectAsrModel([model("small", "installed"), model("large", "installed")], "large", "small")?.id).toBe("large");
  });
  it("falls back to an installed current default", () => {
    expect(selectAsrModel([model("small", "installed"), model("large", "not_installed")], "large", "small")?.id).toBe("small");
  });
  it("never selects an installed legacy ASR", () => {
    expect(
      selectAsrModel(
        [legacyModel("old", "installed"), model("current", "not_installed")],
        "old",
        "old",
      )?.id,
    ).toBe("current");
  });
  it("falls back to any installed ASR", () => {
    expect(selectAsrModel([model("small", "installed"), model("large", "not_installed")], "large", "missing")?.id).toBe("small");
  });
  it("returns the recommended model when installation is required", () => {
    expect(selectAsrModel([model("small", "not_installed"), model("large", "not_installed")], "large", "small")?.id).toBe("large");
  });
});
