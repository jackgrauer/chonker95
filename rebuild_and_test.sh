#!/bin/bash

echo "=== Rebuilding Chonker95 with Image Fix ==="
echo ""

cd /Users/jack/chonker95

# Clean previous build
echo "Cleaning previous build..."
cargo clean

# Build fresh with all the new changes
echo "Building with new image display code..."
cargo build --release

if [ $? -eq 0 ]; then
    echo ""
    echo "✅ Build successful!"
    echo ""
    echo "The new version will use 'kitty +kitten icat' directly"
    echo "since we confirmed that works on your system."
    echo ""
    echo "=== Quick Test ==="
    echo ""
    
    # Test if the image still exists and can be displayed
    if [ -f "/tmp/chonker_kitty_1.jpg" ]; then
        echo "Testing with existing image..."
        kitty +kitten icat --clear --place=50x30@1x1 /tmp/chonker_kitty_1.jpg
        echo ""
        echo "Did you see the PDF page above? ✅"
        sleep 2
        kitty +kitten icat --clear
    fi
    
    echo ""
    echo "=== Now test the app ==="
    echo ""
    echo "Run: ./target/release/chonker95 your_pdf.pdf"
    echo "Then press Ctrl+A to toggle A-B mode"
    echo ""
    echo "The left panel should show the PDF page image!"
    echo ""
    echo "Debug log will show:"
    echo "  'Success: Image displayed via icat' (if using new code)"
    echo "  instead of"
    echo "  'DEBUG: Sent graphics protocol commands' (old code)"
else
    echo "❌ Build failed. Check for errors above."
fi
