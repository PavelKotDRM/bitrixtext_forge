# BitrixText Forge

[Russian version](docs/README.ru.md)

A local desktop application for preparing messages for Bitrix24. Edit source text in Markdown, preview the result, and copy compatible BBCode or plain text for manual insertion into Bitrix24.

The application does not connect to Bitrix24, send messages, or store credentials.

For a step-by-step user guide in Russian, see [the tutorial](docs/tutorial/README.md).

## Features

- Markdown editor with automatic or manual conversion.
- Four output profiles: **Bitrix24 Full Message**, **Bitrix24 Core Safe**, **Plain Text**, and **Manual Code Highlight**.
- AST document preview, generated BBCode, and conversion diagnostics.
- Support for headings, lists (including task lists and nested lists), blockquotes, links, bold, italic, strikethrough, code, images, and horizontal rules.
- Markdown table support with cell formatting preserved and a text fallback: Bitrix24 has no documented BBCode table tag.
- Special inserts: `[u]`, `[user=ID]`, `[color=#HEX]`, `[size=N]`, and `[icon=URL ...]` with parameter validation.
- Open and save Markdown, export the result to text or JSON, export images and tables to a selected folder, and copy plain or formatted results to the system clipboard.
- Built-in and custom templates, autosave, session recovery, and a recent files list.
- All settings, drafts, and templates are stored locally.

## Requirements

- Rust stable with edition 2024 support.
- Windows is the primary platform. Linux and macOS are supported when an environment compatible with `eframe` is available.

## Running

```powershell
cargo run --release
```

For development, use:

```powershell
cargo run
```

## Verification

```powershell
cargo test
cargo build --release
```

## Docker Build

Docker Desktop with Buildx enabled and access to Linux containers is required. The script builds optimized native Linux binaries for `x86_64` and `aarch64`, exporting them to the `dist` directory:

```powershell
.\scripts\build-docker.ps1 -Clean
```

To build a single architecture or change the artifacts directory:

```powershell
.\scripts\build-docker.ps1 -Platform linux/amd64 -OutputDirectory artifacts -Clean
```

The results are placed in `dist\linux-amd64` and `dist\linux-arm64`; the executable name contains the target architecture. Windows is built natively with `cargo build --release`. macOS requires a native Mac: Docker on Linux/Windows does not include the redistributable Apple SDK required to build the application correctly.

## Input and Output

The application opens `.md`, `.markdown`, and `.txt` files. The source is saved as UTF-8 Markdown; the result can be exported to `.txt`, `.bbcode.txt`, or JSON with `markdown`, `bbcode`, and `profile` fields. The separate **📦 Ресурсы** action asks for a destination folder for extracted tables and images.

Full Message generates an extended subset of Bitrix24 BBCode, including `[b]`, `[i]`, `[u]`, `[s]`, `[url]`, `[color]`, `[size]`, `[icon]`, and `[user]`. Core Safe keeps only basic formatting. Plain Text does not generate BBCode.

Full Message wraps fenced code blocks in `[code]`; Core Safe uses a readable
four-space fallback unless `[code]` is explicitly enabled, and Manual Code
Highlight renders code without `[code]`. Images are represented as readable links rather than `[img]`. The resource
export copies local image files, saves external URLs as `.url` shortcuts, and
writes a `resources.txt` manifest without downloading from the network. HTML is
not executed, and unsupported constructs or constructs that may lose formatting
are reported in the diagnostics.

## Technologies

- Rust 2024
- `eframe` / `egui`
- `pulldown-cmark`
- `serde` / `serde_json`
- `rfd` and `arboard`

See the [technical specification](ТЗ.md) for detailed functional requirements.

## License

See [LICENSE](LICENSE).
