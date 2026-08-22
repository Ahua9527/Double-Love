// Purpose: harden packaged Electron binaries with the approved Fuse V1 policy.
// Run: electron-builder calls this file through electron-builder.yml afterPack.
// Requirements: @electron/fuses and a completed electron-builder app directory.

const fs = require('node:fs')
const path = require('node:path')
const { flipFuses, FuseVersion, FuseV1Options } = require('@electron/fuses')

module.exports = async function afterPack(context) {
  const productName = context.packager.appInfo.productFilename
  let executablePath
  if (context.electronPlatformName === 'darwin') {
    const appPath = path.join(context.appOutDir, `${productName}.app`)
    executablePath = path.join(appPath, 'Contents', 'MacOS', productName)
    const frameworkResources = path.join(
      appPath,
      'Contents/Frameworks/Electron Framework.framework/Versions/A/Resources',
    )
    const snapshots = fs.readdirSync(frameworkResources)
      .filter((name) => /^v8_context_snapshot\.[^.]+\.bin$/u.test(name))
    if (snapshots.length !== 1) {
      throw new Error(`Expected one architecture V8 snapshot, found ${snapshots.length}`)
    }
    fs.copyFileSync(
      path.join(frameworkResources, snapshots[0]),
      path.join(frameworkResources, 'browser_v8_context_snapshot.bin'),
    )
  } else if (context.electronPlatformName === 'win32') {
    executablePath = path.join(context.appOutDir, `${productName}.exe`)
  } else {
    executablePath = path.join(context.appOutDir, productName)
  }

  await flipFuses(executablePath, {
    version: FuseVersion.V1,
    resetAdHocDarwinSignature: context.electronPlatformName === 'darwin',
    strictlyRequireAllFuses: true,
    [FuseV1Options.RunAsNode]: false,
    [FuseV1Options.EnableCookieEncryption]: true,
    [FuseV1Options.EnableNodeOptionsEnvironmentVariable]: false,
    [FuseV1Options.EnableNodeCliInspectArguments]: false,
    [FuseV1Options.EnableEmbeddedAsarIntegrityValidation]: true,
    [FuseV1Options.OnlyLoadAppFromAsar]: true,
    [FuseV1Options.LoadBrowserProcessSpecificV8Snapshot]: true,
    [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
    [FuseV1Options.WasmTrapHandlers]: true,
  })
}
