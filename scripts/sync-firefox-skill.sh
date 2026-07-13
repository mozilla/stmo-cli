#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <firefox-checkout> [bug-number]" >&2
    echo "  <firefox-checkout>  path to a local mozilla-firefox/firefox git checkout" >&2
    echo "  [bug-number]        optional Bugzilla bug to reference in the commit message" >&2
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage
    exit 1
fi

firefox_dir="$1"
bug_number="${2:-}"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
src="$repo_root/.claude/skills/stmo/SKILL.md"

if [[ ! -f "$src" ]]; then
    echo "error: vendored skill not found at $src" >&2
    exit 1
fi

if [[ ! -d "$firefox_dir/.git" ]]; then
    echo "error: $firefox_dir does not look like a git checkout (no .git)" >&2
    exit 1
fi

# Firefox mirrors in-tree skills under both .claude/skills and .agents/skills and enforces
# they stay byte-identical via the agent-skills-sync linter (bug 2015814) — every destination
# must be updated together or CI fails.
dest_rel_paths=(
    ".claude/skills/stmo/SKILL.md"
    ".agents/skills/stmo/SKILL.md"
)

changed=0
for rel_path in "${dest_rel_paths[@]}"; do
    dest="$firefox_dir/$rel_path"
    if [[ ! -f "$dest" ]] || ! diff -q "$src" "$dest" >/dev/null 2>&1; then
        changed=1
    fi
done

if [[ "$changed" -eq 0 ]]; then
    echo "SKILL.md is already up to date in $firefox_dir"
    exit 0
fi

for rel_path in "${dest_rel_paths[@]}"; do
    dest="$firefox_dir/$rel_path"
    mkdir -p "$(dirname "$dest")"
    cp "$src" "$dest"
done

stmo_cli_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -1)"

commit_subject="Update stmo skill to match stmo-cli $stmo_cli_version"
if [[ -n "$bug_number" ]]; then
    commit_subject="Bug $bug_number - $commit_subject"
fi

git -C "$firefox_dir" add "${dest_rel_paths[@]}"
git -C "$firefox_dir" commit -m "$commit_subject"

commit_sha="$(git -C "$firefox_dir" rev-parse --short HEAD)"

cat <<EOF

Committed $commit_sha in $firefox_dir:
  $commit_subject

Next step (not run automatically):
  moz-phab submit --no-wip --single $commit_sha --test-plan "Docs-only change: synced ${dest_rel_paths[0]} and ${dest_rel_paths[1]} from stmo-cli $stmo_cli_version via scripts/sync-firefox-skill.sh. No functional change to Firefox."

Then set the "testing-exception-unchanged" Testing Policy project tag on the
revision via the Phabricator web UI (docs-only change, no behavior change).
EOF
