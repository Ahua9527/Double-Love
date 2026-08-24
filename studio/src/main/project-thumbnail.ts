export function projectIdFromThumbnailUrl(value: string): string | null {
  try {
    const url = new URL(value);
    const segments = url.pathname.split("/").filter(Boolean);
    if (url.protocol !== "dl-thumbnail:" || url.hostname !== "project" || segments.length !== 1)
      return null;
    const projectId = decodeURIComponent(segments[0]);
    return /^[A-Za-z0-9._-]{1,128}$/u.test(projectId) ? projectId : null;
  } catch {
    return null;
  }
}
