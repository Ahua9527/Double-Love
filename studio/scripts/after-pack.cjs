// Purpose: harden packaged Electron binaries with the approved Fuse V1 policy.
// Run: electron-builder calls this file through electron-builder.yml afterPack.
// Requirements: @electron/fuses and a completed electron-builder app directory.

const path = require('node:path')
const { flipFuses, FuseVersion, FuseV1Options } = require('@electron/fuses')

module.exports = async function afterPack(context) {
  const productName = context.packager.appInfo.productFilename
  let executablePath
  if (context.electronPlatformName === 'darwin') {
    executablePath = path.join(context.appOutDir, `${productName}.app`, 'Contents', 'MacOS', productName)
  } else if (context.electronPlatformName === 'win32') {
    executablePath = path.join(context.appOutDir, `${productName}.exe`)
  } else {
    executablePath = path.join(context.appOutDir, productName)
  }

  await flipFuses(executablePath, {
    version: FuseVersion.V1,
    [FuseV1Options.RunAsNode]: false,
    [FuseV1Options.EnableCookieEncryption]: true,
    [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
    [FuseV1Options.EnableNodeCliInspectArguments]: false,
    [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
    [FuseV1Options.OnlyLoadAppFromAsar]: true,
  })
}
