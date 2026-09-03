# serverbrowser

A terminal browser built on the [Servo](https://servo.org) web engine with
[qutebrowser](https://qutebrowser.org)-style keyboard navigation and a bookmark
manager backed by an Obsidian-like node mindmap.

## What it is

- **Rendering** — uses the `servo` crate (v0.5.0) embedded in-process with its
  *software* rendering path (`SoftwareRenderingContext`), so it needs no
  window system or GPU. Pages are rendered to RGBA pixels and then emitted to
  the terminal as an inline image.
- **Navigation** — qutebrowser-style keybindings (`j`/`k` scroll, `gg`/`G`
  top/bottom, `gt`/`gT` tabs, `:` command mode, `f` hints) with a prefix-key
  keymap (`gg` style multi-key bindings).
- **Bookmarks as a mindmap** — bookmarks are Markdown nodes in an Obsidian-like
  vault (YAML frontmatter + `[[wikilinks]]`). The graph of nodes and edges is
  stored on disk as plain `.md` files, so it is inspectable and editable
  elsewhere, and a future minimap can just read node/edge data.

## Status

Fully buildable and running on Termux/Android (`aarch64-linux-android`). All unit
tests (config, mindmap, nav, output) pass.

The Servo engine initializes, loads pages (network + JS), the QuteBrowser-style
navigation and the bookmark/mindmap manager all work end-to-end.

Known issue: pixel readback of the software renderer returns a blank (white)
frame on Termux + Mesa. Servo renders through WebRender on Mesa's `zink`
(Gallium-llvmpipe-over-Vulkan) software driver, and `SoftwareRenderingContext`'s
framebuffer readback does not capture the composited output on this specific
driver combination. The whole pipeline (WebRender "generated frame with N
passes", `notify_new_frame_ready` → `paint()`) runs; only the final
`read_to_image` returns a cleared buffer. The `text` output mode is a fallback
that needs no pixel readback, but Servo's `innerText`/`textContent` evaluation
currently returns empty on this build. Resolving the readback (e.g. forcing Mesa
to the pure-Gallium `swrast` instead of `zink`) is the main remaining task.

## Terminal output modes

| Mode | Protocol | Requirements |
|------|----------|--------------|
| `kitty`  | Kitty / iTerm2 graphics protocol | kitty, iTerm2, WezTerm, foot |
| `sixel`  | DEC sixel | xterm(+sixel), mlterm |
| `blocks` | Half-block ANSI truecolor | any modern terminal |
| `text`   | readable text fallback | anything |

Set via `output_mode` in config or the `SERVERBROWSER_OUTPUT` env var.

## Usage

```sh
# Build
cargo build --release

# Render a URL once and print it to the terminal
serverbrowser render https://example.com
SERVERBROWSER_OUTPUT=blocks serverbrowser render https://example.com

# Interactive (opens a URL, `:` command prompt)
serverbrowser open https://example.com

# Bookmarks (mindmap nodes)
serverbrowser bookmark-add https://doc.rust-lang.org
serverbrowser bookmark-add https://crates.io RustDocs    # link under a parent title
serverbrowser bookmarks
```

## qutebrowser-style commands

- `:open URL`, `:open -t URL` (new tab)
- `:back`, `:forward`, `:reload`
- `:bookmark-add <URL> [parent]`, `:bookmarks`
- `:quit`

Default keybindings (`normal` mode):

| Key | Action | Key | Action |
|-----|--------|-----|--------|
| `j`/`k`/`h`/`l` | scroll | `:` `o` `b` `B` | command entry |
| `gg`/`G` | top/bottom | `gt`/`gT` | next/prev tab |
| `H`/`L` | back/forward | `r` | reload |
| `f` | hints | `yy` | yank URL |
| `+`/`-` | zoom | `q` | quit |

## Configuration

`~/.config/serverbrowser/config.toml` (TOML). Key fields:

```toml
start_page = "about:blank"
output_mode = "kitty"
viewport_width = 1280
viewport_height = 800
vault_dir = "~/.local/share/serverbrowser/vault"
```

## The mindmap / bookmark vault

Each bookmark is a Markdown file in `vault_dir`:

```markdown
---
title: "Doc Rust"
url: "https://doc.rust-lang.org"
tags: rust, docs
---

Some notes here.

- [[crates]]
```

`[[wikilinks]]` become edges in the mindmap graph. The `bookmarks` command
prints nodes and edges; the interactive minimap overlay is a planned follow-up
(see below).

## Roadmap / planned

- Interactive TUI minimap rendering the node graph (the "node minimap").
- Link-hints overlay (`f` to follow links).
- Full tab management and persistent sessions.
- Search-engine and quickmark shortcut queries.

## Building on Termux / Android

Servo normally targets a desktop userspace; on Termux (whose Rust host triple is
`aarch64-linux-android`) one C dependency of webrender — `glslopt` — includes
the NDK header `<log/log.h>`, which Termux ships as `android/log.h`. A no-op
shim is provided in `shims/log/log.h` and injected via `.cargo/config.toml`.

## License

MPL-2.0 (matching Servo).