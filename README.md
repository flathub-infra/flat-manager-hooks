# Flathub Hooks

This is a collection of scripts intended to integrate Flathub's backend with its flat-manager instance. It uses the
hooks feature of flat-manager, which runs commands at certain points in the upload/build/publish process.

These scripts are specific to Flathub. They aren't intended to be generalizable, but if you're running your own
instance of flat-manager you can specify your own set of scripts to do something similar.

All of the hooks are built as the same Rust binary and called using subcommands.

## flathub-hooks publish

This hook is run *during* the publish job. It fetches information about the app from the backend and edits the
build's commits to match. It updates appstream data, commit subsets, and token type.

## flathub-hooks review

This is the hook for reviewing a build. It checks with the backend for changes in appstream metadata and requests
a moderator review if necessary. It also runs some validators on the uploaded commits and reports any warnings or
errors to flat-manager.