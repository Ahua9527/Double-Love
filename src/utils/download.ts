export function getOutputFileName(originalName: string, duplicateNumber = 1): string {
  const baseName = originalName.replace(/\.xml$/i, '');
  const suffix = duplicateNumber > 1 ? `_${duplicateNumber}` : '';
  return `${baseName}_Double_LOVE${suffix}.xml`;
}
