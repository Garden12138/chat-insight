# Third-party notices

Chat Insight uses the following third-party software. JavaScript and Rust package licenses are also
recorded in their respective lock files and package metadata.

## wechat-cli

- Project: https://github.com/r266-tech/wechat-cli
- Pinned release: `v1.6.20`
- License: MIT
- Distribution: the complete upstream macOS arm64 or Windows amd64 runtime subset is downloaded by
  the release workflow, verified against the SHA-256 digest published in GitHub release metadata,
  and included only in supported installers.

The upstream package's `LICENSE`, `SECURITY.md`, and `THIRD_PARTY_NOTICES.md` are copied into the
application resources when present. macOS Intel builds do not include this component.

Chat Insight does not modify the upstream binaries and does not enable message sending, UI control,
or a background watcher.
