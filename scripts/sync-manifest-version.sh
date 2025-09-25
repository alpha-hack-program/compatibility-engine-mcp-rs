#!/bin/bash

# Pre-release hook for cargo-release
# Syncs the version from Cargo.toml to dxt/manifest.json and .env

set -e  # Exit on any error

echo "🔄 Syncing version from Cargo.toml to dxt/manifest.json and .env..."

# Get version from Cargo.toml using cargo metadata
if command -v jq &> /dev/null; then
    VERSION=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[0].version')
else
    # Fallback to grep if jq is not available
    VERSION=$(grep '^version = ' Cargo.toml | head -1 | cut -d'"' -f2)
fi

if [ -z "$VERSION" ] || [ "$VERSION" == "null" ]; then
    echo "❌ Error: Could not extract version from Cargo.toml!"
    exit 1
fi

echo "📦 Found version: $VERSION"

# Update dxt/manifest.json
if [ -f "dxt/manifest.json" ]; then
    if command -v jq &> /dev/null; then
        # Use jq for robust JSON editing
        jq --arg version "$VERSION" '.version = $version | .dxt_version = $version' dxt/manifest.json > dxt/manifest.json.tmp && mv dxt/manifest.json.tmp dxt/manifest.json
        echo "✅ Updated dxt/manifest.json with version $VERSION"
    else
        echo "⚠️  jq not found. Please install jq for robust JSON editing:"
        echo "   macOS: brew install jq"
        echo "   Ubuntu: sudo apt install jq"
        exit 1
    fi
else
    echo "⚠️  dxt/manifest.json not found - skipping"
fi

# Update .env file with VERSION
if [ -f ".env" ]; then
    # Update existing .env file
    if grep -q "^VERSION=" .env; then
        # Replace existing VERSION line
        if [[ "$OSTYPE" == "darwin"* ]]; then
            # macOS sed syntax
            sed -i '' "s/^VERSION=.*/VERSION=$VERSION/" .env
        else
            # Linux sed syntax
            sed -i "s/^VERSION=.*/VERSION=$VERSION/" .env
        fi
        echo "✅ Updated .env VERSION to $VERSION"
    else
        # Add VERSION to existing .env
        echo "VERSION=$VERSION" >> .env
        echo "✅ Added VERSION=$VERSION to .env"
    fi
else
    # Create new .env file
    echo "VERSION=$VERSION" > .env
    echo "✅ Created .env with VERSION=$VERSION"
fi

echo "🎉 Version sync complete!"
