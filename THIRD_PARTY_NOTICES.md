# Third-party notices

Chat Insight uses the following third-party software. JavaScript and Rust package licenses are also
recorded in their respective lock files and package metadata.

## wechat-cli

- Project: https://github.com/r266-tech/wechat-cli
- Pinned release: `v1.6.20`
- License: MIT
- Distribution: release builds compile the fixed Go module source archive on the native target
  runner after verifying the archive against a repository-pinned SHA-256.

The macOS bridge also includes `r266-tech/wxkey v1.4.8` (MIT). The encrypted SQLite runtime is built
from Tencent WCDB v2.1.15 (BSD 3-Clause). Their license and notice files are copied into application
resources when present. macOS Intel builds do not include these components.

Chat Insight does not modify the upstream sources and does not enable message sending, UI control,
or a background watcher.
