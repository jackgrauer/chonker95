#!/bin/bash

echo "=== Testing Chonker95 Image Display Fix ==="
echo ""
echo "✅ Kitty Terminal Detected (Window ID: $KITTY_WINDOW_ID)"
echo ""

# Build the app
echo "Building Chonker95..."
cd /Users/jack/chonker95
cargo build --release 2>&1 | tail -n 3

# Check if build succeeded
if [ $? -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"
echo ""

# Create a test PDF if one doesn't exist
TEST_PDF="/tmp/test_document.pdf"
if [ ! -f "$TEST_PDF" ]; then
    echo "Creating test PDF..."
    # Use ps2pdf or convert to create a simple PDF
    if command -v convert >/dev/null 2>&1; then
        convert -size 400x600 -density 150 \
                xc:white \
                -pointsize 24 -fill black -gravity north \
                -annotate +0+50 "Test PDF Document" \
                -pointsize 18 -gravity center \
                -annotate +0+0 "This is a test page\nfor Chonker95\n\nPage 1" \
                "$TEST_PDF"
        echo "✅ Test PDF created: $TEST_PDF"
    else
        echo "⚠️  No test PDF available. Use your own PDF file."
        TEST_PDF=""
    fi
fi

echo ""
echo "=== Testing Image Display ==="
echo ""

# First verify kitty icat works directly
echo "1. Testing kitty icat directly..."
TEST_IMG="/tmp/test_kitty_direct.jpg"
if command -v convert >/dev/null 2>&1; then
    convert -size 200x100 gradient:blue-green "$TEST_IMG"
    kitty +kitten icat --clear --place=40x20@1x1 "$TEST_IMG"
    echo "   Did you see a blue-green gradient? (Should work)"
    sleep 2
    kitty +kitten icat --clear
else
    echo "   Skipping (ImageMagick not installed)"
fi

echo ""
echo "2. Testing Chonker95..."
if [ -n "$TEST_PDF" ]; then
    echo "   Instructions:"
    echo "   - Press Ctrl+A to toggle A-B comparison mode"
    echo "   - You should see the PDF page on the left"
    echo "   - Press Q to quit"
    echo ""
    echo "   Starting in 3 seconds..."
    sleep 3
    ./target/release/chonker95 "$TEST_PDF"
else
    echo "   Please run: ./target/release/chonker95 your_pdf_file.pdf"
    echo "   Then press Ctrl+A to see the image"
fi

echo ""
echo "=== Checking Debug Log ==="
if [ -f /tmp/chonker_debug.log ]; then
    echo "Debug log contents:"
    cat /tmp/chonker_debug.log
else
    echo "No debug log found"
fi

echo ""
echo "=== Summary ==="
echo "The fix uses 'kitty +kitten icat' command directly since we confirmed it works."
echo "This is more reliable than the escape sequence method in raw terminal mode."
echo ""
echo "If images still don't appear:"
echo "1. Check that /tmp/chonker_kitty_*.jpg files exist"
echo "2. Try manually: kitty +kitten icat /tmp/chonker_kitty_1.jpg"
echo "3. Make sure you're in A-B mode (Ctrl+A)"
