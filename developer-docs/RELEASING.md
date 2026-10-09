# Releasing procinsh

A **push of an annotated version tag** triggers `.github/workflows/release.yml`.
GitHub Actions builds the frontend, tests and validates the crate, publishes it to
crates.io, then creates a GitHub Release using the **handwritten annotated tag
message verbatim**. No generated release notes are appended.

## One-time setup

As an owner of the existing `procinsh` crate, configure a **GitHub Actions
Trusted Publisher** in its crates.io settings:

- GitHub owner: `akawashiro`
- Repository: `procinsh`
- Workflow filename: `release.yml`
- Environment: leave unset (the workflow does not use a GitHub Environment)

This allows `rust-lang/crates-io-auth-action@v1` to obtain a short-lived
publishing credential without a permanent `CARGO_REGISTRY_TOKEN` secret.

## Each release

1. Update the `procinsh` package version in **both** `Cargo.toml` and its
   `[[package]]` entry in `Cargo.lock`. Do not update dependency versions
   unnecessarily. Commit this change on `main` (or merge a PR), and push it.
2. Create an **annotated** tag at the release commit. Git opens your editor
   so you can write the release notes in Markdown:
   
   ```sh
   git switch main
   git pull --ff-only
   git tag -a v0.1.13
   # Write release notes in the editor, then save and close.
   git push origin v0.1.13
   ```

   Alternatively, to write a longer draft in a file (the file does **not**
   need to be committed):

   ```sh
   git tag -a v0.1.13 -F /path/to/my-release-notes.md
   git push origin v0.1.13
   ```

3. Watch the **Release** workflow in GitHub Actions. The GitHub Release will
   appear after crates.io publishing succeeds.

Replace `0.1.13` with the actual new version; this is only an example.

## Guardrails and failure recovery

- Tags must match `vMAJOR.MINOR.PATCH` (optional SemVer prerelease suffix).
- Only annotated tags are accepted, and their message must be nonempty.
- The tagged commit must already be reachable from `main`.
- The tag, `Cargo.toml`, and `Cargo.lock` versions must agree.
- The existing `scripts/check_web_package.py` verifies that generated web
  assets are packaged and the extracted crate builds without Node.js.
- Publishing runs before GitHub Release creation. If the crate was published
  but GitHub Release creation failed, rerun the failed **github-release**
  job; do not republish the same immutable crates.io version.
- Do **not** move or force-push an already-published release tag. If a release
  contains a defect, make a new version instead.

To preview the exact handwritten notes before pushing:

```sh
git for-each-ref --format='%(contents)' refs/tags/v0.1.13
```
