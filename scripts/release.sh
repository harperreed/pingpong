#!/bin/bash
# ABOUTME: GitHub release automation script for version tagging and publishing
# ABOUTME: Handles version bumping, changelog generation, and tag creation

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Check if we're in a git repository
if ! git rev-parse --git-dir > /dev/null 2>&1; then
    error "Not in a git repository"
fi

# Check if working directory is clean
if ! git diff-index --quiet HEAD --; then
    error "Working directory is not clean. Please commit or stash your changes."
fi

# Get current version from Cargo.toml
current_version=$(grep "^version" Cargo.toml | sed 's/version = "\(.*\)"/\1/')
info "Current version: $current_version"

# Parse version type argument
if [ $# -eq 0 ]; then
    echo "Usage: $0 <major|minor|patch|VERSION>"
    echo "  major: X.0.0"
    echo "  minor: X.Y.0"
    echo "  patch: X.Y.Z"
    echo "  VERSION: specific version (e.g., 1.2.3)"
    exit 1
fi

version_type=$1

# Calculate new version
if [[ "$version_type" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    # Specific version provided
    new_version=$version_type
else
    # Parse current version
    IFS='.' read -r major minor patch <<< "$current_version"
    
    case $version_type in
        major)
            new_version="$((major + 1)).0.0"
            ;;
        minor)
            new_version="$major.$((minor + 1)).0"
            ;;
        patch)
            new_version="$major.$minor.$((patch + 1))"
            ;;
        *)
            error "Invalid version type: $version_type"
            ;;
    esac
fi

info "New version: $new_version"

# Confirm release
read -p "Create release v$new_version? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    info "Release cancelled"
    exit 0
fi

# Update version in Cargo.toml
info "Updating Cargo.toml version"
sed -i.bak "s/^version = \".*\"/version = \"$new_version\"/" Cargo.toml
rm -f Cargo.toml.bak

# Update Cargo.lock
info "Updating Cargo.lock"
cargo check

# Commit version bump
info "Committing version bump"
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to $new_version

🤖 Generated with [Claude Code](https://claude.ai/code)

Co-Authored-By: Claude <noreply@anthropic.com>"

# Create and push tag
info "Creating tag v$new_version"
git tag -a "v$new_version" -m "Release version $new_version"

info "Pushing changes and tag"
git push origin main
git push origin "v$new_version"

info "Release v$new_version created successfully!"
info "GitHub Actions will automatically build and publish the release."
info "Check the Actions tab on GitHub for build progress."