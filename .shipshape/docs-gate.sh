#!/bin/sh
# shipshape docs gate: fails when code changed but the required memory docs did not.
# Input: the changed file paths, one per line, on stdin.
#   commit-msg hook:  git diff --cached --name-only | sh .shipshape/docs-gate.sh
#   CI (a PR):        git diff --name-only origin/main...HEAD | sh .shipshape/docs-gate.sh
# Settings come from .shipshape/config (code_paths, docs_required). No config = nothing to check.

root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
config="$root/.shipshape/config"
[ -f "$config" ] || exit 0

cfg() { sed -n "s/^$1=//p" "$config" | tail -n 1 | tr -d '\r'; }
code_paths=$(cfg code_paths)
docs_required=$(cfg docs_required)
[ -n "$docs_required" ] || docs_required="docs/STATUS.md docs/CHANGELOG.md"
[ -n "$code_paths" ] || exit 0

files=$(cat | tr -d '\r')

code=$(printf '%s\n' "$files" | while IFS= read -r f; do
    [ -n "$f" ] || continue
    for p in $code_paths; do
        case "$f" in "$p"*) printf '%s\n' "$f"; break ;; esac
    done
done)
[ -n "$code" ] || exit 0

missing=""
for d in $docs_required; do
    printf '%s\n' "$files" | grep -qxF "$d" || missing="$missing $d"
done
[ -n "$missing" ] || exit 0

{
    echo ""
    echo "shipshape: code changed but the project memory was not updated."
    echo "  missing:$missing"
    echo ""
    echo "  code files in this change:"
    printf '%s\n' "$code" | head -n 10 | sed 's/^/    /'
    echo ""
    echo "  Update the docs (an agent: use the shipshape-memory skill), stage them with the code, and"
    echo "  commit again. A change with no behaviour change: put [no-docs] in the commit message."
} >&2
exit 1
