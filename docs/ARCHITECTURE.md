# Architecture (as built)

> The map of what actually exists. Update it whenever modules, data, interfaces, commands or flows change.
> Only real paths and names: every one here must exist in the code.

## Runtime

Nothing runs yet. The planned architecture is in `blueprint.md` §3 and §5.

## Code map

| Path | Responsibility |
|---|---|
| `blueprint.md` | Product plan (v1); `archive/blueprint-v0.md` is the original draft |
| `DESIGN.md`, `design/tokens.css` | Design system and its tokens |
| `.shipshape/`, `.githooks/` | Workflow config and git hooks |

## Commands

```
git config core.hooksPath .githooks   # once per clone
```
