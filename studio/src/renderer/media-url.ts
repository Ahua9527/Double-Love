export function mediaAssetUrl(assetId: string) {
  return `dl-media://asset/${encodeURIComponent(assetId)}`
}
