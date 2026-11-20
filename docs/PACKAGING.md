# Packaging

What a release produces today, and what is still missing.

## Shipped on a `v*` tag

| Artefact | Platform | How |
| --- | --- | --- |
| `daydoc-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 | `release.yml`, `taiki-e/upload-rust-binary-action` |
| `daydoc-vX.Y.Z-x86_64-pc-windows-msvc.zip` | Windows 10/11 | same workflow, MSVC target |
| `daydoc_X.Y.Z_amd64.deb` | Debian/Ubuntu | `cargo deb`, `[package.metadata.deb]` |

Every release job runs `cargo test --all-targets` before it packages anything, so
a tag cannot ship an untested binary.

## Cutting a release

```bash
# bump version in Cargo.toml and move CHANGELOG [Unreleased] into a new section
cargo test --all-targets
git commit -am "release: v0.3.0"
git tag v0.3.0
git push origin main --tags
```

`0.2.0` is prepared in `Cargo.toml` and `CHANGELOG.md` and is waiting for its
tag.

## Locally

```bash
cargo build --release            # target/release/daydoc
cargo install cargo-deb
cargo deb                        # target/debian/daydoc_*.deb
```

## Not yet built

| Artefact | Needs |
| --- | --- |
| Windows MSI | `cargo-wix` and a hand-checked `wix/main.wxs`; the manifest must be validated on Windows |
| AppImage | `linuxdeploy` plus a `.desktop` file and an icon |
| Self-update | `self_update` against GitHub Releases, with a signature check |
| Portable `.exe` | Already covered by the Windows zip; a single-file build would need static CRT |

The release profile is already sized for this: `opt-level = "z"`, `lto = "fat"`,
`codegen-units = 1`, `panic = "abort"`, `strip = true`.
