#!/bin/bash
# Build script for Windows 11 executable

set -e

echo "Building auto-backup.exe for Windows 11..."
echo ""

# Ensure the Windows target is installed
if ! rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
    echo "Installing Windows target..."
    rustup target add x86_64-pc-windows-gnu
    echo ""
fi

# Build for Windows
echo "Compiling..."
cargo build --release --target x86_64-pc-windows-gnu

echo ""
echo "✓ Build completed successfully!"
echo ""
echo "Windows executable location:"
echo "  $(pwd)/target/x86_64-pc-windows-gnu/release/auto-backup.exe"
echo ""
echo "File size: $(du -h target/x86_64-pc-windows-gnu/release/auto-backup.exe | cut -f1)"
