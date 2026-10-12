#!/usr/bin/env python3
"""Build and validate the website and agent artifacts from the same sources."""

import argparse
import html
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs"
STAGING = ROOT / "target/docs-source"
SITE = ROOT / "target/docs-site"
BASE_URL = "https://nuzzles.github.io/mcpstack/"
MDBOOK_VERSION = "0.5.4"


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


class Links(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.ids = set()
        self.links = []
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        for key in ("href", "src"):
            if attrs.get(key):
                self.links.append(attrs[key])


def check_links():
    documents = {
        path: Links(path.read_text(encoding="utf-8"))
        for path in SITE.rglob("*.html")
    }
    for source, document in documents.items():
        for link in document.links:
            url = urlsplit(link)
            if url.scheme or url.netloc:
                continue
            path = unquote(url.path)
            if path.startswith("/mcpstack/"):
                target = SITE / path.removeprefix("/mcpstack/")
            elif path.startswith("/"):
                raise ValueError(f"Link escapes the Pages project: {source.name}: {link}")
            else:
                target = source.parent / path if path else source
            target = target.resolve()
            if target.is_dir():
                target /= "index.html"
            if not target.is_relative_to(SITE) or not target.is_file():
                raise ValueError(f"Broken local link: {source.name}: {link}")
            if url.fragment and target in documents:
                if unquote(url.fragment) not in documents[target].ids:
                    raise ValueError(f"Broken anchor: {source.name}: {link}")
    print(f"Checked links and anchors in {len(documents)} HTML pages.")


def check_examples(binary, pages, env):
    with tempfile.TemporaryDirectory(prefix="mcpstack-doc-examples-") as directory:
        temp = Path(directory)
        count = 0
        for page in pages:
            for yaml in re.findall(r"```yaml\n(.*?)```", page.read_text(), re.S):
                stack = temp / "example.yaml"
                stack.write_text(yaml, encoding="utf-8")
                run(str(binary), "validate", str(stack), env=env, capture_output=True)
                count += 1
        stack = SOURCE / "src/examples/demo.yaml"
        run(str(binary), "validate", str(stack), env=env, capture_output=True)
        for client, extension in [("codex", "toml"), ("claude", "json")]:
            config = temp / f"scratch.{extension}"
            args = [str(binary), client]
            target = [str(stack), "--config", str(config), "--non-interactive"]
            before = set(temp.iterdir())
            for operation in ("diff", "import", "use"):
                flags = [] if operation == "diff" else ["--dry-run", "-y"]
                run(*args, operation, *target, *flags, env=env, capture_output=True)
            assert set(temp.iterdir()) == before, "Preview wrote files"
            run(*args, "import", *target, "-y", env=env, capture_output=True)
            original = {path: path.read_bytes() for path in temp.iterdir()}
            for operation in ("import", "use"):
                run(*args, operation, *target, "-y", env=env, capture_output=True)
                assert original == {path: path.read_bytes() for path in temp.iterdir()}
            run(*args, "list", "--config", str(config), env=env, capture_output=True)
        print(f"Validated {count + 1} YAML examples and both scratch-config workflows.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mdbook", default="mdbook", help="mdBook executable")
    parser.add_argument("--binary", type=Path, help="Use an already-built mcpstack executable")
    args = parser.parse_args()
    version = run(args.mdbook, "--version", capture_output=True).stdout.strip()
    if version != f"mdbook v{MDBOOK_VERSION}":
        raise ValueError(f"Expected mdBook {MDBOOK_VERSION}; got {version}")
    binary = args.binary
    if binary is None:
        run("cargo", "build", "--locked", cwd=ROOT)
        binary = ROOT / "target/debug" / ("mcpstack.exe" if os.name == "nt" else "mcpstack")
    binary = binary.resolve()
    env = os.environ.copy()
    env.pop("RUST_LOG", None)
    env["NO_COLOR"] = "1"

    # Stage generated reference files separately; a build never edits docs sources.
    shutil.rmtree(STAGING, ignore_errors=True)
    shutil.rmtree(SITE, ignore_errors=True)
    shutil.copytree(SOURCE / "src", STAGING / "src")
    shutil.copytree(SOURCE / "theme", STAGING / "theme")
    shutil.copyfile(SOURCE / "book.toml", STAGING / "book.toml")
    schema = json.loads(run(str(binary), "--schema", env=env, capture_output=True).stdout)
    cli_version = schema["cli_version"]
    revision = run("git", "rev-parse", "HEAD", cwd=ROOT, capture_output=True).stdout.strip()
    reference = [
        "# CLI reference\n",
        f"Generated from mcpstack **{cli_version}** at site build time. Your installed version's "
        "`--help` and `--schema` are authoritative for that version.\n",
        "Download the [CLI schema](cli-schema.json) for structured command metadata.\n",
    ]

    def describe(command, prefix):
        help_text = run(str(binary), *prefix, "--help", env=env, capture_output=True).stdout
        reference.append(f"## mcpstack {' '.join(prefix)}\n\n```text\n{help_text.rstrip()}\n```\n")
        for child in command["commands"]:
            if child["name"] != "help":
                describe(child, [*prefix, child["name"]])

    describe(schema["command"], [])
    reference.append("## Exit statuses\n\n| Status | Code | Meaning |\n| --- | --- | --- |")
    for status in schema["exit_statuses"]:
        reference.append(f"| {status['status']} | {status['error_code'] or 'SUCCESS'} | {status['description']} |")
    (STAGING / "src/cli-reference.md").write_text("\n".join(reference) + "\n", encoding="utf-8")
    (STAGING / "src/cli-schema.json").write_text(json.dumps(schema, indent=2) + "\n", encoding="utf-8")
    names = re.findall(r"\]\(([^)]+\.md)\)", (STAGING / "src/SUMMARY.md").read_text())
    pages = [STAGING / "src" / name for name in names]
    check_examples(binary, pages, env)
    run(args.mdbook, "build", str(STAGING), "--dest-dir", str(SITE))

    index = [
        f"# mcpstack {cli_version}\n",
        "> Manage, share, and switch MCP server configurations across Codex and Claude Code.\n",
        f"Documentation for mcpstack {cli_version}. Permissions are opt-in; "
        "use replaces the entire server set. For unattended import previews, "
        "use --non-interactive -y.\n",
        f"Source revision: https://github.com/nuzzles/mcpstack/commit/{revision}\n",
        "## Documentation\n",
    ]
    full = [f"# mcpstack {cli_version} documentation\n", f"Source revision: {revision}\n"]
    for name, page in zip(names, pages):
        markdown = page.read_text(encoding="utf-8")
        title = markdown.splitlines()[0].removeprefix("# ")
        (SITE / name).write_text(markdown, encoding="utf-8")
        index.append(f"- [{title}]({BASE_URL}{name})")
        # Make chapter links usable even in the aggregated document.
        markdown = re.sub(
            r"\]\((?![a-z]+:|#)([^)]+)\)",
            lambda match: f"]({BASE_URL}{match[1]})",
            markdown,
        )
        full.append(f"\n---\n\nSource: {BASE_URL}{name}\n\n{markdown}")
        rendered = SITE / Path(name).with_suffix(".html")
        document = rendered.read_text(encoding="utf-8")
        raw = html.escape(Path(name).name, quote=True)
        document = document.replace("</head>", f'<link rel="alternate" type="text/markdown" href="{raw}">\n</head>')
        if "<main>" not in document:
            raise ValueError(f"Cannot add Markdown link to {rendered}")
        document = document.replace("<main>", f'<main><div class="page-tools"><a href="{raw}">Read Markdown ↗</a></div>', 1)
        # The generated chapter has no editable source page.
        if name == "cli-reference.md":
            document = document.replace("/edit/main/docs/src/cli-reference.md", "/edit/main/docs/build.py")
        rendered.write_text(document, encoding="utf-8")
    index.extend([
        "\n## Machine-readable reference\n",
        f"- [Complete Markdown guide]({BASE_URL}llms-full.txt)",
        f"- [CLI schema JSON]({BASE_URL}cli-schema.json)",
    ])
    (SITE / "llms.txt").write_text("\n".join(index) + "\n", encoding="utf-8")
    (SITE / "llms-full.txt").write_text("\n".join(full) + "\n", encoding="utf-8")
    (SITE / ".nojekyll").touch()
    check_links()
    print(f"Built {len(pages)} chapters, Markdown sources, llms.txt, llms-full.txt, and CLI schema in {SITE}")


if __name__ == "__main__":
    main()
