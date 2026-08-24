{
  "targets": [{
    "target_name": "avfoundation_player",
    "sources": ["src/player.mm"],
    "xcode_settings": {
      "CLANG_ENABLE_OBJC_ARC": "YES",
      "MACOSX_DEPLOYMENT_TARGET": "15.0",
      "OTHER_CPLUSPLUSFLAGS": ["-std=c++20"]
    },
    "libraries": [
      "-framework AppKit",
      "-framework AVFoundation",
      "-framework CoreMedia",
      "-framework QuartzCore",
      "-framework VideoToolbox"
    ]
  }]
}
