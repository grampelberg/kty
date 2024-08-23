# Releases

Push a new tag to main and the `release` workflow will take care of the rest.

Note: versions are automatically managed as part of the github workflows, see
`just set-version` for what's actually happening. If you need a version replaced
in a file, set it to `just --evaluate version_placeholder`.
