# Notterox's Zed fork

A personal build of [Zed](https://github.com/zed-industries/zed) with features Zed lacks.
Nothing here is meant for upstream; upstream releases are merged in from time to time.

## Branches and remotes

- `origin` = <https://github.com/Notterox/zed>, `upstream` = `zed-industries/zed`.
- `main`: this fork's version. It is based on an upstream **stable release tag** (currently
  `v1.26.2`), never on upstream `main`.
- `feat/<name>`: one branch per feature, cut from `main` and merged back with `--no-ff`.
- The fork also carries copies of upstream's branches from when it was created; they are not
  used. The local clone fetches only `origin/main`, `origin/feat/*`, `upstream/main` and tags
  (blob-less partial clone: file contents download on demand).

## Rules for changes

- Put new logic in new files or crates. Edits to upstream files are small hooks, each marked
  with a `// NOTTEROX:` comment, so `git grep NOTTEROX` lists every touch point.
- Every feature has tests, listed below, and an entry in this file.
- Keep GitHub Actions disabled on the fork: upstream's CI is heavy and not set up here.

## Features

### Code actions popup: groups, search, editor commands (`feat/code-actions-popup`)

The inline code actions menu (`editor::ToggleCodeActions`, `ctrl-.` / `alt-enter`):

- Rows are grouped under headers by LSP code action kind: Quick Fixes, Refactor, Extract,
  Inline, Source, then Navigate, Run, Debug, Other. Headers show only when more than one
  group has rows; selection skips them.
- Typing while the menu is open fuzzy-filters it (synchronously, `fuzzy_nucleo`). A
  keystroke interceptor consumes printable keys and backspace before key bindings, so this
  works in vim normal mode too; a leading space still reaches space-leader bindings.
- Opened at the cursor in a buffer with a language, the menu also lists Rename, Find All
  References, Go to Implementation, Go to Type Definition, Show Incoming Calls, Organize
  Imports and Format Document.

Files:

- `crates/editor/src/code_actions_menu_rows.rs` (new): all of the logic and row rendering.
- Hooks: `crates/editor/src/editor.rs` (module), `crates/editor/src/code_context_menus.rs`
  (`CodeActionsMenu` fields, selection, `visible`, `render`, `render_aside`,
  `CodeActionContents::iter` made `pub(crate)`), `crates/editor/src/code_actions.rs`
  (`toggle_code_actions` builds the rows, `confirm_code_action` runs command rows),
  `crates/editor/Cargo.toml` (`fuzzy_nucleo`).

Tests: `editor` `editor_tests::test_code_actions_menu_groups_and_search`, `vim`
`test::test_code_actions_menu_search_in_normal_mode`.

## Building and installing

Prerequisites are in `docs/src/development/windows.md` (MSVC with Spectre libs, Windows SDK,
CMake; the script uses the CMake bundled with Visual Studio 2026).

```bash
script/notterox-install            # release build, installs to C:/Home/Soft/ZedFork
```

Close the Zed running from the install folder first; Windows locks `Zed.exe` while it runs.

The installed layout matches the official one: `Zed.exe` plus the CLI as `bin/zed.exe`.
`C:\Home\Soft\ZedFork\bin` is on the user `PATH` (this is the only Zed installed), so `zed`
in a terminal and the Unity package's discovery both find the fork. The script also writes a
per-user Start menu shortcut, `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Zed.lnk`.

The official installer also ships `conpty.dll`, `amd_ags_x64.dll` and `x64/OpenConsole.exe`
(Microsoft and AMD binaries it downloads at bundle time). The script keeps the copies already
in the install folder; for a fresh folder, unpack them from an official installer of the same
version, e.g. with Scoop's `innounp -x -d<dir> Zed-x86_64.exe`, or set `ZED_STOCK_DIR` to an
official install. Without them the terminal falls back to the system ConPTY. Development builds:
`cargo build -p zed` (`target/debug/zed.exe`, slow at runtime). First builds take ~14 min
(debug) and ~21 min (release).

## Merging an upstream release

1. `git fetch upstream --tags`
2. `git switch main && git merge vX.Y.Z` (a stable tag). Resolve conflicts with the file
   lists above; `git grep NOTTEROX` shows every hook.
3. Run each feature's tests, e.g.
   `cargo test -p editor --lib code_action` and
   `cargo test -p vim --lib test_code_actions_menu_search_in_normal_mode`.
4. `script/notterox-install`, use it for a while, then `git push origin main`.
5. Update the base tag under "Branches and remotes".
