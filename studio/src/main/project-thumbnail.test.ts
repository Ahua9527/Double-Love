import { describe, expect, it } from "vitest";
import { projectIdFromThumbnailUrl } from "./project-thumbnail";

describe("project thumbnail URL", () => {
  it("accepts only a single opaque project id", () => {
    expect(projectIdFromThumbnailUrl("dl-thumbnail://project/project-1?v=2")).toBe("project-1");
    expect(projectIdFromThumbnailUrl("dl-thumbnail://project/a_b.c")).toBe("a_b.c");
  });

  it("rejects paths, foreign hosts, traversal and malformed ids", () => {
    expect(projectIdFromThumbnailUrl("dl-thumbnail://asset/project-1")).toBeNull();
    expect(projectIdFromThumbnailUrl("dl-thumbnail://project/a/b")).toBeNull();
    expect(projectIdFromThumbnailUrl("dl-thumbnail://project/%2E%2E")).toBeNull();
    expect(projectIdFromThumbnailUrl("file:///private/video.mov")).toBeNull();
  });
});
