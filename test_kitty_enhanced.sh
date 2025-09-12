#!/bin/bash

# Enhanced test script for Kitty graphics protocol
echo "=== Kitty Graphics Protocol Test ==="
echo "TERM: $TERM"
echo "TERM_PROGRAM: $TERM_PROGRAM"
echo ""

# Check if we're in Kitty
if [[ "$TERM" != *"kitty"* ]] && [[ "$TERM_PROGRAM" != "kitty" ]]; then
    echo "⚠️  WARNING: Not running in Kitty terminal"
    echo "Graphics display will not work. Please use Kitty terminal."
    exit 1
fi

echo "✅ Running in Kitty terminal"
echo ""

# Create a test image using Ghostscript (similar to what your app does)
TEST_PDF="/System/Library/AssetsV2/com_apple_MobileAsset_Font7/4a074c38b09c56f88e4b901d58eed93c96a92398.asset/AssetData/Fonts/Supplemental/Arial Unicode.ttf"
TEST_IMAGE="/tmp/kitty_test_image.jpg"

echo "Creating test image..."
# Create a simple image using ImageMagick if available
if command -v convert >/dev/null 2>&1; then
    convert -size 400x200 xc:'#4A90E2' \
            -pointsize 30 -fill white -gravity center \
            -annotate +0+0 "Kitty Graphics Test\n✅ Working!" \
            "$TEST_IMAGE"
    echo "Test image created with ImageMagick"
elif command -v gs >/dev/null 2>&1 && [ -f "$TEST_PDF" ]; then
    # Fallback to using Ghostscript if we have a PDF
    gs -dNOPAUSE -dBATCH -dSAFER -dQUIET \
       -sDEVICE=jpeggray -dFirstPage=1 -dLastPage=1 \
       -r150 -dJPEGQ=85 -sOutputFile="$TEST_IMAGE" \
       "$TEST_PDF" 2>/dev/null
    echo "Test image created with Ghostscript"
else
    echo "Neither ImageMagick nor Ghostscript available"
    echo "Please install ImageMagick: brew install imagemagick"
    exit 1
fi

if [ ! -f "$TEST_IMAGE" ]; then
    echo "❌ Failed to create test image"
    exit 1
fi

echo "✅ Test image created: $TEST_IMAGE"
echo "File size: $(ls -lh "$TEST_IMAGE" | awk '{print $5}')"
echo ""

# Test different methods
echo "=== Testing Display Methods ==="
echo ""

# Method 1: Direct file transmission (simplest)
echo "Method 1: Direct file path transmission"
echo "Command: printf '\\e_Gt=f,a=T;/tmp/kitty_test_image.jpg\\e\\\\'"
printf '\e_Gt=f,a=T;%s\e\\' "$TEST_IMAGE"
sleep 1
echo ""
echo ""

# Method 2: With size constraints
echo "Method 2: With size constraints (30 cols x 15 rows)"
echo "Command: printf '\\e_Gt=f,a=T,c=30,r=15;/tmp/kitty_test_image.jpg\\e\\\\'"
printf '\e[2J\e[H'  # Clear screen and move to top
printf '\e_Gt=f,a=T,c=30,r=15;%s\e\\' "$TEST_IMAGE"
sleep 1
echo ""
echo ""

# Method 3: Base64 encoded path
echo "Method 3: Base64 encoded path"
ENCODED_PATH=$(echo -n "$TEST_IMAGE" | base64)
echo "Command: printf '\\e_Ga=T,t=t,f=100;[base64_encoded_path]\\e\\\\'"
printf '\e_Ga=T,t=t,f=100;%s\e\\' "$ENCODED_PATH"
sleep 1
echo ""
echo ""

# Method 4: Using kitty icat if available
if command -v kitty >/dev/null 2>&1; then
    echo "Method 4: Using kitty icat command"
    kitty +kitten icat --place=40x20@0x25 "$TEST_IMAGE"
    echo ""
fi

echo ""
echo "=== Test Complete ==="
echo ""
echo "If you saw images above, Kitty graphics are working!"
echo "If not, check:"
echo "1. You're using Kitty terminal (not just SSH into a machine)"
echo "2. Graphics aren't disabled in Kitty settings"
echo "3. Try running: kitty +kitten icat $TEST_IMAGE"
echo ""
echo "Debug info saved to: /tmp/kitty_test_debug.log"

# Save debug info
{
    echo "Debug Information - $(date)"
    echo "TERM=$TERM"
    echo "TERM_PROGRAM=$TERM_PROGRAM"
    echo "Test image: $TEST_IMAGE"
    echo "Image exists: $([ -f "$TEST_IMAGE" ] && echo "Yes" || echo "No")"
    echo "Image size: $(ls -l "$TEST_IMAGE" 2>/dev/null | awk '{print $5}') bytes"
    echo "Kitty version: $(kitty --version 2>/dev/null || echo "Not found")"
} > /tmp/kitty_test_debug.log
