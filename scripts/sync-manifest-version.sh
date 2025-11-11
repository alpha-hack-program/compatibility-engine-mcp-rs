#!/bin/bash

# Pre-release hook for cargo-release
# Syncs the version from Cargo.toml to mcpb/manifest.json, .env, and Containerfile
# Also updates Rust source code with .env values

set -e  # Exit on any error

echo "🔄 Syncing version from Cargo.toml to mcpb/manifest.json, .env, Containerfile, and Rust source..."

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

# Update mcpb/manifest.json
if [ -f "mcpb/manifest.json" ]; then
    if command -v jq &> /dev/null; then
        # Use jq for robust JSON editing
        jq --arg version "$VERSION" '.version = $version' mcpb/manifest.json > mcpb/manifest.json.tmp && mv mcpb/manifest.json.tmp mcpb/manifest.json
        echo "✅ Updated mcpb/manifest.json with version $VERSION"
    else
        echo "⚠️  jq not found. Please install jq for robust JSON editing:"
        echo "   macOS: brew install jq"
        echo "   Ubuntu: sudo apt install jq"
        exit 1
    fi
else
    echo "⚠️  mcpb/manifest.json not found - skipping"
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
    # Throw an error
    echo "❌ Error: .env file not found!"
    exit 1
fi

# Read values from .env file for Rust source replacement
echo "🔧 Reading .env values for Rust source code replacement..."

# Function to expand variable references in a value
expand_vars() {
    local value=$1
    local visited_vars="${2:-}"  # Track visited variables to prevent circular dependencies
    local expanded=$value
    
    # Find all ${VAR} patterns in the value
    while [[ $expanded =~ \$\{([^}]+)\} ]]; do
        local var_ref="${BASH_REMATCH[1]}"
        local var_value
        
        # Check for circular dependency
        if [[ "$visited_vars" =~ ":$var_ref:" ]]; then
            echo "Warning: Circular dependency detected for variable $var_ref" >&2
            break
        fi
        
        # Read the variable value from .env
        if [ -f ".env" ]; then
            var_value=$(grep "^${var_ref}=" .env | cut -d'=' -f2- | sed 's/^"//' | sed 's/"$//')
        fi
        
        # If variable not found in .env, try environment variables
        if [ -z "$var_value" ]; then
            var_value="${!var_ref:-}"
        fi
        
        # If still empty, leave the reference as-is
        if [ -z "$var_value" ]; then
            break
        fi
        
        # Recursively expand the variable value
        local new_visited="${visited_vars}:${var_ref}:"
        var_value=$(expand_vars "$var_value" "$new_visited")
        
        # Replace the reference with the expanded value
        expanded=${expanded//\$\{$var_ref\}/$var_value}
    done
    
    echo "$expanded"
}

# Function to read .env variables
read_env_var() {
    local var_name=$1
    local default_value=$2
    local value
    
    if [ -f ".env" ]; then
        value=$(grep "^${var_name}=" .env | cut -d'=' -f2- | sed 's/^"//' | sed 's/"$//')
    fi
    
    if [ -z "$value" ]; then
        value=$default_value
    else
        # Expand variable references in the value
        value=$(expand_vars "$value")
    fi
    
    echo "$value"
}

# Get values from .env
ENGINE_NAME=$(read_env_var "ENGINE_NAME" "ERROR_ENGINE_NAME")
APP_NAME=$(read_env_var "APP_NAME" "ERROR_APP_NAME")
ENV_VERSION=$(read_env_var "VERSION" "$VERSION")  # Use VERSION from Cargo.toml as fallback
TITLE=$(read_env_var "TITLE" "ERROR_TITLE")
SOURCE=$(read_env_var "SOURCE" "ERROR_SOURCE")
# DESCRIPTION: use from .env if available, otherwise construct from TITLE
DESCRIPTION=$(read_env_var "DESCRIPTION" "${TITLE} - Model Context Protocol server")

echo "📋 .env values:"
echo "   APP_NAME: $APP_NAME"
echo "   VERSION: $ENV_VERSION" 
echo "   TITLE: $TITLE"
echo "   ENGINE_NAME: $ENGINE_NAME"
echo "   SOURCE: $SOURCE"
echo "   DESCRIPTION: $DESCRIPTION"

# Check if ENGINE_NAME is valid
if [ "$ENGINE_NAME" == "ERROR_ENGINE_NAME" ]; then
    echo "❌ Error: ENGINE_NAME is not set in .env!"
    exit 1
fi

# Check if APP_NAME is valid
if [ "$APP_NAME" == "ERROR_APP_NAME" ]; then
    echo "❌ Error: APP_NAME is not set in .env!"
    exit 1
fi

# Check if TITLE is valid
if [ "$TITLE" == "ERROR_TITLE" ]; then
    echo "❌ Error: TITLE is not set in .env!"
    exit 1
fi

# Check if SOURCE is valid
if [ "$SOURCE" == "ERROR_SOURCE" ]; then
    echo "❌ Error: SOURCE is not set in .env!"
    exit 1
fi

# DESCRIPTION is optional - if not in .env, it will be constructed from TITLE

# Update Containerfile with VERSION, APP_NAME, SOURCE, and DESCRIPTION from .env
if [ -f "Containerfile" ]; then
    # Escape special characters for sed (using | as delimiter, so we need to escape | and \)
    # Function to escape sed special characters when using | as delimiter
    escape_sed() {
        echo "$1" | sed 's/\\/\\\\/g' | sed 's/|/\\|/g'
    }
    
    ESCAPED_APP_NAME=$(escape_sed "$APP_NAME")
    ESCAPED_SOURCE=$(escape_sed "$SOURCE")
    ESCAPED_DESCRIPTION=$(escape_sed "$DESCRIPTION")
    
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS sed syntax - use | as delimiter to avoid conflicts with / in URLs
        sed -i '' "s|^ARG VERSION=.*|ARG VERSION=$ENV_VERSION|" Containerfile
        sed -i '' "s|^ARG APP_NAME=.*|ARG APP_NAME=$ESCAPED_APP_NAME|" Containerfile
        sed -i '' "s|^ARG SOURCE=.*|ARG SOURCE=$ESCAPED_SOURCE|" Containerfile
        sed -i '' "s|^ARG DESCRIPTION=.*|ARG DESCRIPTION=\"$ESCAPED_DESCRIPTION\"|" Containerfile
    else
        # Linux sed syntax - use | as delimiter to avoid conflicts with / in URLs
        sed -i "s|^ARG VERSION=.*|ARG VERSION=$ENV_VERSION|" Containerfile
        sed -i "s|^ARG APP_NAME=.*|ARG APP_NAME=$ESCAPED_APP_NAME|" Containerfile
        sed -i "s|^ARG SOURCE=.*|ARG SOURCE=$ESCAPED_SOURCE|" Containerfile
        sed -i "s|^ARG DESCRIPTION=.*|ARG DESCRIPTION=\"$ESCAPED_DESCRIPTION\"|" Containerfile
    fi
    echo "✅ Updated Containerfile with VERSION=$ENV_VERSION, APP_NAME=$APP_NAME, SOURCE=$SOURCE, DESCRIPTION=$DESCRIPTION"
else
    echo "⚠️  Containerfile not found - skipping"
fi

# ENGINE_NAME must replace '-' with '_' in the Rust source code
ENGINE_NAME=$(echo "$ENGINE_NAME" | tr '-' '_')

# Update Rust source code
RUST_FILE="src/common/${ENGINE_NAME}.rs"
if [ -f "$RUST_FILE" ]; then
    echo "🦀 Updating Rust source code with .env values..."
    
    # Create a backup
    cp "$RUST_FILE" "$RUST_FILE.bak"
    
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS sed syntax - use | as delimiter to avoid conflicts with / in URLs
        sed -i '' "s|let name = \"[^\"]*\"\.to_string();|let name = \"$APP_NAME\".to_string();|" "$RUST_FILE"
        sed -i '' "s|let version = \"[^\"]*\"\.to_string();|let version = \"$ENV_VERSION\".to_string();|" "$RUST_FILE"
        sed -i '' "s|let title = \"[^\"]*\"\.to_string();|let title = \"$TITLE\".to_string();|" "$RUST_FILE"
        sed -i '' "s|let website_url = \"[^\"]*\"\.to_string();|let website_url = \"$SOURCE\".to_string();|" "$RUST_FILE"
    else
        # Linux sed syntax - use | as delimiter to avoid conflicts with / in URLs
        sed -i "s|let name = \"[^\"]*\"\.to_string();|let name = \"$APP_NAME\".to_string();|" "$RUST_FILE"
        sed -i "s|let version = \"[^\"]*\"\.to_string();|let version = \"$ENV_VERSION\".to_string();|" "$RUST_FILE"
        sed -i "s|let title = \"[^\"]*\"\.to_string();|let title = \"$TITLE\".to_string();|" "$RUST_FILE"
        sed -i "s|let website_url = \"[^\"]*\"\.to_string();|let website_url = \"$SOURCE\".to_string();|" "$RUST_FILE"
    fi
    
    echo "✅ Updated $RUST_FILE with .env values"
    echo "💾 Backup saved as $RUST_FILE.bak"
else
    echo "⚠️  $RUST_FILE not found - skipping Rust source update"
fi

echo "🎉 Version sync complete!"
