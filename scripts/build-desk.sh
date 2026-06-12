#!/usr/bin/env bash
set -euo pipefail

# Build the real Frappe Desk frontend assets for open_frappe
# Prerequisites: Node.js >= 24, yarn (or npm), git

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
# Frappe esbuild expects: bench/apps/frappe/ and bench/sites/
APPS_DIR="$PROJECT_ROOT/apps"
ASSETS_DIR="$PROJECT_ROOT/crates/http/assets"
FRAPPE_REPO="https://github.com/frappe/frappe.git"
FRAPPE_BRANCH="version-16"

echo "=== Frappe Desk Build ==="
echo "Project root: $PROJECT_ROOT"
echo "Apps dir:     $APPS_DIR"
echo "Assets dir:   $ASSETS_DIR"

# Add common Homebrew node@24 path if present (keg-only formula)
if [ -d "/opt/homebrew/opt/node@24/bin" ]; then
    export PATH="/opt/homebrew/opt/node@24/bin:$PATH"
fi

# Check prerequisites
command -v node >/dev/null 2>&1 || { echo "ERROR: Node.js is required but not installed. Install Node.js >= 24 first."; exit 1; }
command -v yarn >/dev/null 2>&1 || { echo "ERROR: yarn is required but not installed. Install yarn first."; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq is required but not installed. Install jq first."; exit 1; }

NODE_VERSION=$(node --version | sed 's/v//')
NODE_MAJOR=$(echo "$NODE_VERSION" | cut -d. -f1)
if [ "$NODE_MAJOR" -lt 24 ]; then
    echo "ERROR: Node.js >= 24 is required. Found: $NODE_VERSION"
    exit 1
fi

# Ensure sites/apps.txt exists (tells Frappe which apps to build)
mkdir -p "$PROJECT_ROOT/sites/assets"
if [ ! -f "$PROJECT_ROOT/sites/apps.txt" ]; then
    echo "frappe" > "$PROJECT_ROOT/sites/apps.txt"
fi

# Clean and prepare — only remove frappe-built assets, preserve our files
rm -rf "$ASSETS_DIR/frappe"
rm -f "$ASSETS_DIR/assets.json"
mkdir -p "$ASSETS_DIR"

# Clone or update frappe into apps/frappe (standard bench layout)
FRAPPE_DIR="$APPS_DIR/frappe"
if [ -d "$FRAPPE_DIR/.git" ]; then
    echo "Updating frappe repository..."
    git -C "$FRAPPE_DIR" fetch origin
    git -C "$FRAPPE_DIR" checkout "$FRAPPE_BRANCH"
    git -C "$FRAPPE_DIR" pull origin "$FRAPPE_BRANCH"
else
    echo "Cloning frappe repository (branch: $FRAPPE_BRANCH)..."
    rm -rf "$FRAPPE_DIR"
    mkdir -p "$APPS_DIR"
    git clone --depth 1 --branch "$FRAPPE_BRANCH" "$FRAPPE_REPO" "$FRAPPE_DIR"
fi

# Install dependencies and build
cd "$FRAPPE_DIR"
echo "Installing Node dependencies..."
yarn install --frozen-lockfile

echo "Building Frappe desk assets..."
yarn build

# Copy built assets to our project
# Frappe v15 builds to sites/assets/frappe/dist/
FRAPPE_DIST="$PROJECT_ROOT/sites/assets/frappe/dist"
if [ -d "$FRAPPE_DIST" ]; then
    echo "Copying built assets from $FRAPPE_DIST ..."
    mkdir -p "$ASSETS_DIR/frappe/dist"
    cp -r "$FRAPPE_DIST"/* "$ASSETS_DIR/frappe/dist/"
else
    echo "WARNING: Expected dist directory not found at $FRAPPE_DIST"
    echo "Searching for built assets..."
    find "$PROJECT_ROOT/sites" -type d -name "dist" | head -5
    exit 1
fi

# Also copy essential static files (images, fonts, etc.)
FRAPPE_PUBLIC="$FRAPPE_DIR/frappe/public"
for subdir in images icons fonts; do
    if [ -d "$FRAPPE_PUBLIC/$subdir" ]; then
        echo "Copying $subdir ..."
        cp -r "$FRAPPE_PUBLIC/$subdir" "$ASSETS_DIR/$subdir"
    fi
done

# Copy Frappe's assets.json which maps bundle names to hashed filenames
echo "Copying assets.json ..."
cp "$PROJECT_ROOT/sites/assets/assets.json" "$ASSETS_DIR/assets.json"

echo ""
echo "=== Build Complete ==="
echo "Assets available at: $ASSETS_DIR"
echo "assets.json:"
cat "$ASSETS_DIR/assets.json"
echo ""
echo "Next: run 'cargo run' to start the server with the real Frappe Desk."
