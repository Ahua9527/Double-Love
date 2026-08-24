import type { ModelDescriptor } from "./platform/desktop";

export function selectAsrModel(
  models: ModelDescriptor[],
  recommendedId: string,
  defaultModelId?: string,
): ModelDescriptor | null {
  const asr = models.filter(
    (model) => model.kind === "asr" && model.ui_role !== "legacy",
  );
  const installed = (id: string | undefined) =>
    id ? asr.find((model) => model.id === id && model.state === "installed") : undefined;
  return (
    installed(recommendedId) ??
    installed(defaultModelId) ??
    asr.find((model) => model.state === "installed") ??
    asr.find((model) => model.id === recommendedId) ??
    asr.find((model) => model.id === defaultModelId) ??
    asr[0] ??
    null
  );
}
