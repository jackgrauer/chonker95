#!/bin/bash

echo "=== Rebuilding Chonker95 with Fixed Display ==="
echo ""
echo "Key fixes:"
echo "- Removed alternate screen (was conflicting with kitty graphics)"
echo "- Fixed screen clearing logic"
echo "- Simplified image display"
echo ""

cd /Users/jack/chonker95

# Clean rebuild
echo "Clean rebuild..."
cargo clean
cargo build --release 2>&1 | tail -n 3

if [ $? -ne 0 ]; then
    echo "Build failed!"
    exit 1
fi

echo "✅ Build successful!"
echo ""
echo "Testing..."

# Create test PDF if needed
TEST_PDF="/tmp/test.pdf"
if ! [ -f "$TEST_PDF" ]; then
    echo "Creating test PDF..."
    echo "Test Document" | ps2pdf - "$TEST_PDF"
fi

echo ""
echo "=== Running Chonker95 ==="
echo ""
echo "Instructions:"
echo "1. Press Ctrl+A to toggle A-B mode (image should appear on left)"
echo "2. The image should stay visible without ANSI code spray"
echo "3. Press Ctrl+A again to return to text mode"
echo "4. Press Q to quit"
echo ""
echo "Starting in 3 seconds..."
sleep 3

./target/release/chonker95 "$TEST_PDF"

echo ""
echo "=== Debug Log ==="
cat /tmp/chonker_debug.log 2>/dev/null || echo "No debug log"
echo ""
echo "If the image still doesn't work, try this manual test:"
echo "kitty +kitten icat --clear --place=50x30@1x1 /tmp/chonker_kitty_1.jpg"
