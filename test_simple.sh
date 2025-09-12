#!/bin/bash

echo "✅ Confirmed: You're in Kitty terminal (Window ID: $KITTY_WINDOW_ID)"
echo ""

# Create a simple test image with ImageMagick or use a fallback
TEST_IMG="/tmp/kitty_simple_test.jpg"

if command -v convert >/dev/null 2>&1; then
    # Create a colorful test image
    convert -size 400x200 gradient:blue-yellow \
            -pointsize 30 -fill black -gravity center \
            -annotate +0-20 "Kitty Graphics Working!" \
            -pointsize 20 -fill white -gravity center \
            -annotate +0+20 "Window ID: $KITTY_WINDOW_ID" \
            "$TEST_IMG"
    echo "Created test image with ImageMagick"
else
    # Create a minimal valid JPEG if ImageMagick isn't available
    # This is a small red square
    base64 -d > "$TEST_IMG" << 'EOF'
/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAIBAQIBAQICAgICAgICAwUDAwMDAwYEBAMFBwYHBwcG
BwcICQsJCAgKCAcHCg0KCgsMDAwMBwkODw0MDgsMDAz/2wBDAQICAgMDAwYDAwYMCAcIDAwMDAwM
DAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAz/wAARCABAAEADASIA
AhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQA
AAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3
ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWm
p6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEA
AwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSEx
BhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElK
U1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3
uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD+/iii
igAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKK
ACiiigD/2Q==
EOF
    echo "Created fallback test image"
fi

if [ ! -f "$TEST_IMG" ]; then
    echo "❌ Could not create test image"
    exit 1
fi

echo "Test image: $TEST_IMG ($(stat -f%z "$TEST_IMG" 2>/dev/null || stat -c%s "$TEST_IMG" 2>/dev/null || echo "unknown size") bytes)"
echo ""
echo "=== Testing Kitty Graphics Protocol ==="
echo ""

# The simplest, most compatible way to display an image in Kitty
echo "Sending image to terminal..."
printf '\033_Gf=100,a=T;%s\033\\' "$TEST_IMG"

echo ""
echo ""
echo "Did you see an image? If yes, graphics are working!"
echo ""
echo "If NO image appeared, try these debugging steps:"
echo ""
echo "1. Check Kitty config (~/.config/kitty/kitty.conf):"
echo "   Ensure you don't have: allow_remote_control no"
echo ""
echo "2. Try manually with kitty icat:"
echo "   kitty +kitten icat $TEST_IMG"
echo ""
echo "3. Check if you're in SSH/tmux/screen:"
echo "   These can interfere with graphics protocol"
echo ""
echo "4. Try the latest Kitty version:"
echo "   kitty --version"
echo "   Current: $(kitty --version 2>/dev/null || echo 'kitty not in PATH')"
