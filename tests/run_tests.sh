#!/bin/bash

MANIFESTS=$(find . -name "*.yml" -type f)

FAILURES=0

mkdir -p repos
mkdir -p build_dirs

for MANIFEST in *.yml ;
do
    APP_ID=$(basename "$MANIFEST" .yml)

    echo
    echo "===== Running test $MANIFEST ====="
    echo

    flatpak-builder --mirror-screenshots-url=https://dl.flathub.org/media "--repo=repos/$APP_ID" --force-clean "build_dirs/$APP_ID" "$APP_ID.yml"

    if [ $? -ne 0 ]; then
        echo "Build failed"
        exit 1
    fi

    if [ "$APP_ID" != "com.example.NoScreenshotBranch" ]; then
        ostree commit "--repo=repos/$APP_ID" --canonical-permissions --branch=screenshots/x86_64 "build_dirs/$APP_ID/screenshots"
        if [ $? -ne 0 ]; then
            echo "Failed to commit x86_64 screenshots"
            exit 1
        fi
        ostree commit "--repo=repos/$APP_ID" --canonical-permissions --branch=screenshots/aarch64 "build_dirs/$APP_ID/screenshots"
        if [ $? -ne 0 ]; then
            echo "Failed to commit aarch64 screenshots"
            exit 1
        fi
    fi

    cd "repos/$APP_ID"

    if [ "$APP_ID" == "com.example.WrongArchExecutable" ]; then
        rm -rf refs/heads/app/com.example.WrongArchExecutable/aarch64
        cp -r refs/heads/app/com.example.WrongArchExecutable/x86_64 refs/heads/app/com.example.WrongArchExecutable/aarch64
    fi

    cargo run -- validate > validation_result.json
    RESULT=$?

    cd -

    if [ $RESULT -ne 0 ]; then
        echo "Validation command failed with exit code $RESULT"
        exit $RESULT
    fi

    diff -u "$APP_ID.expected.json" "repos/$APP_ID/validation_result.json"

    if [ $? -ne 0 ]; then
        echo "Test $MANIFEST validation results differ from expected"
        FAILURES=$((FAILURES+1))
        continue
    fi

    echo "Test $MANIFEST passed"
done

if [ $FAILURES -ne 0 ]; then
    echo "$FAILURES tests failed"
    exit 1
fi