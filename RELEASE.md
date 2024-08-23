# Releases

Push a new tag to main and the `release` workflow will take care of the rest.

Note: versions are automatically managed as part of the github workflows, see
`just set-version` for what's actually happening. If you need a version replaced
in a file, set it to `just --evaluate version_placeholder`.

## Versioning

The version used for builds is the output from `just --evaluate version`. This
runs `git cliff --bumped-version` and appends `-UNSTABLE` if `HEAD` does not
have a tag pointing at it. This means that for releases which are tagged, the
version results in the tag (ex `v0.0.1` -> `0.0.1`). For releases which are not
tagged, assuming that last tag is `v0.0.1` become `v0.0.2-UNSTABLE`. Note that
`git cliff` has some logic around bumping major and minor versions. Take a look
at their documentation to understand when a major version bump may happen.
