# Documentation site

The site is built with **mdBook 0.5.4** and published at
<https://nuzzles.github.io/mcpstack/> after documentation changes merge to `main`.

## Build and preview

Install [mdBook 0.5.4](https://github.com/rust-lang/mdBook/releases/tag/v0.5.4),
Python 3.11 or newer, and the Rust toolchain required by the root Cargo.toml.
From the repository root:

```sh
python3 docs/build.py
python3 -m http.server 8000 --directory target/docs-site
```

Open <http://localhost:8000/>. Rebuild and refresh after edits. Use
`--mdbook /path/to/mdbook` if it is not on your PATH. `--binary /path/to/mcpstack`
uses an already-built binary; by default the script builds the current checkout
with `cargo build --locked`.

## Edit the docs

Edit `docs/src/*.md` and the navigation in `docs/src/SUMMARY.md`. The build expands
the `command-pages` marker into a subsection for every operation from the CLI
schema, with both harnesses' help on the same page. Edit shared command examples
and explanations in `docs/command-guides/*.md`. Usage and options come from each
command's `--help`. Section 2 is Compatibility, with Claude Code and Codex guides
in `docs/src/harness-*.md`; section 3 is Commands.

Keep runnable examples synthetic and use scratch configuration paths. The build validates
YAML snippets, exercises both clients against scratch configs, and checks local
HTML links and anchors.

The build stages files in `target/docs-source`, generates the CLI reference and
JSON schema from the executable, and produces `target/docs-site`. Do not edit or
commit generated artifacts. Each chapter is also published as Markdown, and the
same content produces `llms.txt` and `llms-full.txt` for agents.
The CLI reference and agent indexes include the version reported by the executable.
Keep the guide's version labels in sync with `Cargo.toml` when preparing a release.

## Publishing

The documentation workflow builds and checks PRs. Only pushes to `main` or a manual
run on `main` can deploy, using GitHub Actions as the Pages source. The `github-pages`
environment controls documentation deployment.

To bootstrap a fork, enable **Settings → Pages → Build and deployment → GitHub
Actions**, and update the repository URLs in `docs/book.toml` and `docs/build.py`.
`site-url` and `BASE_URL` must include the repository subpath. Upgrade the mdBook
version, download checksum in the workflow, and `MDBOOK_VERSION` together.
