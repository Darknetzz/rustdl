# Repository split (2026-06)

This repository was extracted from the personal monorepo [Darknetzz/code](https://github.com/Darknetzz/code) with:

```bash
git filter-repo --path Rust/rustdl/ --path-rename Rust/rustdl/:
```

**216 commits** of history under `Rust/rustdl/` were preserved (paths rewritten to the repo root).

## Remotes

| Remote | URL |
|--------|-----|
| `github` | `git@github.com:Darknetzz/rustdl.git` |
| `gitlab` | `ssh://git@gitlab.roste.org/kriss/rustdl.git` |

Push to both after changes:

```bash
git push github main
git push gitlab main
git push github --tags
git push gitlab --tags
```

## Monorepo

Development no longer happens under `Rust/rustdl/` in `code`. That tree was removed; see `Rust/README.md` in the monorepo for the pointer.
