#!/bin/sh
# shipshape session report. Run it at the start of every agent session (Claude Code's shipshape plugin runs it
# automatically); everything printed is meant for the agent's context. It supplies what static docs cannot:
# live git state, and whether the memory docs have fallen behind the code. It must stay fast and never fail.
# Project-specific lines (servers, live mode, jobs in flight): put them in .shipshape/context.local.sh.

root=$(git rev-parse --show-toplevel 2>/dev/null)
if [ -z "$root" ]; then
    echo "## Project state (shipshape)"
    echo "Not a git repository yet. A new project starts with the shipshape-adopt skill (new-project flow)."
    exit 0
fi
cd "$root" || exit 0
config=.shipshape/config
cfg() { [ -f "$config" ] && sed -n "s/^$1=//p" "$config" | tail -n 1 | tr -d '\r'; }

default=$(cfg default_branch)
[ -n "$default" ] || default=main
code_paths=$(cfg code_paths)
branch=$(git branch --show-current 2>/dev/null)

echo "## Project state (shipshape session report)"
echo ""
echo "**Branch:** ${branch:-detached HEAD} (default: $default)"
echo ""
if git rev-parse --verify -q HEAD >/dev/null 2>&1; then
    echo "**Recent commits:**"
    git log --oneline -8 2>/dev/null | sed 's/^/- /'
else
    echo "**Commits:** none yet (new repository)"
fi
echo ""

dirty=$(git status --porcelain 2>/dev/null)
if [ -n "$dirty" ]; then
    count=$(printf '%s\n' "$dirty" | wc -l | tr -d ' ')
    echo "**Uncommitted changes ($count)** - work was in progress; inspect before starting anything new:"
    printf '%s\n' "$dirty" | head -n 25 | sed 's/^/    /'
else
    echo "**Working tree:** clean"
fi
echo ""

warn=""
add() { warn="$warn
- $1"; }

# Memory docs present?
missing=""
for d in docs/STATUS.md docs/DECISIONS.md docs/ARCHITECTURE.md docs/CHANGELOG.md; do
    [ -f "$d" ] || missing="$missing $d"
done
[ -n "$missing" ] && add "Project memory is missing:$missing. Set it up from the real code (shipshape-adopt / shipshape-memory) before writing code."

# On the default branch?
if [ -n "$branch" ] && { [ "$branch" = "$default" ] || [ "$branch" = main ] || [ "$branch" = master ]; } && git rev-parse --verify -q HEAD >/dev/null 2>&1; then
    add "You are on the default branch '$branch'. Before the first change, ask the user to create a branch (suggest a name such as feat/<topic>); never commit here."
fi

# Unpushed commits
ahead=$(git rev-list --count '@{u}..HEAD' 2>/dev/null)
[ -n "$ahead" ] && [ "$ahead" -gt 0 ] 2>/dev/null && add "$ahead commit(s) are not pushed yet (git push)."

# Docs behind the code?
if [ -f docs/STATUS.md ] && [ -n "$code_paths" ]; then
    status_commit=$(git log -1 --format=%H -- docs/STATUS.md 2>/dev/null)
    if [ -z "$status_commit" ]; then
        add "docs/STATUS.md has never been committed."
    else
        # shellcheck disable=SC2086
        newer=$(git log --oneline "$status_commit..HEAD" -- $code_paths 2>/dev/null)
        if [ -n "$newer" ]; then
            n=$(printf '%s\n' "$newer" | wc -l | tr -d ' ')
            list=$(printf '%s\n' "$newer" | head -n 5 | tr '\n' ';' | sed 's/;$//; s/;/; /g')
            add "docs/STATUS.md is older than $n code commit(s): $list. Bring the docs up to date (shipshape-memory) before new work."
        fi
    fi
    if [ -n "$dirty" ]; then
        dirty_code=$(printf '%s\n' "$dirty" | cut -c4- | while IFS= read -r f; do for p in $code_paths; do case "$f" in "$p"*) echo "$f"; break ;; esac; done; done)
        dirty_status=$(printf '%s\n' "$dirty" | cut -c4- | grep -x 'docs/STATUS.md')
        [ -n "$dirty_code" ] && [ -z "$dirty_status" ] && add "Uncommitted code changes exist but docs/STATUS.md is untouched."
    fi
fi

# Hooks active?
if [ -d .githooks ]; then
    hp=$(git config --get core.hooksPath 2>/dev/null)
    [ "$hp" = ".githooks" ] || add "The repo's git hooks are NOT active in this clone (core.hooksPath='$hp'). Run: git config core.hooksPath .githooks"
fi

# A report from an unattended run that nobody has read yet?
[ -f .shipshape/last-report.md ] && add "An unattended run left a report: .shipshape/last-report.md. Summarise it for the user."

if [ -n "$warn" ]; then
    echo "**ATTENTION before writing code:**$warn"
else
    echo "**Memory docs:** in sync with the latest code commit."
fi
echo ""
echo "Before changing code: read docs/STATUS.md, docs/DECISIONS.md and docs/ARCHITECTURE.md, then the code you will touch."

[ -f .shipshape/context.local.sh ] && . ./.shipshape/context.local.sh
exit 0
