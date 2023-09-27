#!/usr/bin/env nu


# Make sure we've committed all our changes.
let GIT_STATUS = (git status --porcelain  | lines)
if (($GIT_STATUS | length) != 0) { 
    print $"(ansi red_bold) [Error](ansi reset): Working directory must be clean"
    print $"(ansi blue_bold)~~~ Git Status ~~~.(ansi reset)"
    print $GIT_STATUS
    print $"(ansi red_bold)Release Aborted.(ansi reset)"
    exit 1;
}

let CARGO_TOML_VERSION = (open Cargo.toml | get "package" | get "version")

print $"(ansi blue_bold) [INFO]:(ansi reset) Cargo.toml Release Version is: (ansi green_bold)($CARGO_TOML_VERSION)(ansi reset)"

let EXISTING_VERSION_CONFLICT = (git tag | lines | str substring 1.. | any {|line| $line == $CARGO_TOML_VERSION})

if $EXISTING_VERSION_CONFLICT { 
    print $"(ansi red_bold) [ERROR](ansi reset): Version ($CARGO_TOML_VERSION) is already an existing git tag. Consider updating Cargo.toml. (ansi red_bold) Aborting(ansi reset)."
    exit 1;
}

## so if we're up to date locally, and we don't have a tag conflict

## we can 
## 1. Create a tag

git tag $"v($CARGO_TOML_VERSION)"

## 2. Push that tag to github to start ci

git push --tags