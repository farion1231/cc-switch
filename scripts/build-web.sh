#!/bin/bash
set -e

echo "Building CC Switch Web Frontend..."

# Build only the renderer (not the full Tauri app)
pnpm run build:renderer

# Copy the built files to src-tauri/web-dist
echo "Copying web assets to src-tauri/web-dist..."
rm -rf src-tauri/web-dist
mkdir -p src-tauri/web-dist
cp -r dist/* src-tauri/web-dist/

echo "Web frontend build complete!"
echo "Assets available at: src-tauri/web-dist/"
