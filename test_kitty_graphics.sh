#!/bin/bash

# Test script for Kitty graphics protocol
echo "Testing Kitty Graphics Protocol..."
echo "TERM=$TERM"
echo "TERM_PROGRAM=$TERM_PROGRAM"
echo ""

# Create a simple test image if it doesn't exist
TEST_IMAGE="/tmp/test_kitty_image.jpg"
if [ ! -f "$TEST_IMAGE" ]; then
    echo "Creating test image..."
    # Use ImageMagick to create a simple test image
    convert -size 200x100 xc:blue -pointsize 20 -fill white -gravity center -annotate +0+0 "KITTY TEST" "$TEST_IMAGE" 2>/dev/null || {
        echo "Could not create test image. Please ensure ImageMagick is installed."
        echo "Or create your own test image at: $TEST_IMAGE"
    }
fi

if [ -f "$TEST_IMAGE" ]; then
    echo "Test image: $TEST_IMAGE"
    echo ""
    
    # Method 1: Direct file reference
    echo "Method 1: Direct file reference"
    printf '\e_Gt=f,a=T;%s\e\\' "$TEST_IMAGE"
    sleep 1
    echo ""
    
    # Method 2: Using kitty icat (if available)
    echo "Method 2: Using kitty icat"
    if command -v kitty >/dev/null 2>&1; then
        kitty +kitten icat "$TEST_IMAGE"
    else
        echo "kitty command not found"
    fi
    
    echo ""
    echo "If you see an image above, Kitty graphics are working."
    echo "If not, check:"
    echo "1. You're using Kitty terminal"
    echo "2. Graphics are enabled in Kitty config (allow_remote_control yes)"
    echo "3. The image file exists and is readable"
else
    echo "Test image not found at $TEST_IMAGE"
fi
