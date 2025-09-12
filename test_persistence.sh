#!/bin/bash

echo "=== Testing Why Image Doesn't Stay ==="
echo ""

# The issue might be that the image is displayed but then immediately cleared
# when the app continues rendering. Let's test this theory.

echo "Test 1: Simple image display that should stay visible"
echo "-----------------------------------------------"

# Create a test image if it doesn't exist
if [ ! -f /tmp/chonker_kitty_1.jpg ]; then
    if command -v convert >/dev/null 2>&1; then
        convert -size 400x300 plasma: /tmp/chonker_kitty_1.jpg
    fi
fi

if [ -f /tmp/chonker_kitty_1.jpg ]; then
    # Display image with the EXACT command the app uses
    echo "Displaying image with app's exact command..."
    kitty +kitten icat --clear --place=50x30@1x1 --scale-up '/tmp/chonker_kitty_1.jpg'
    
    echo ""
    echo "Image should be visible above."
    echo "Now let's simulate what happens in the app..."
    echo ""
    sleep 2
    
    echo "Simulating terminal clear (like the app does)..."
    # This simulates what happens when the app re-renders
    printf '\033[2J\033[H'  # Clear screen and move cursor home
    
    echo "Did the image disappear? That's the problem!"
    echo ""
    sleep 2
    
    echo "Test 2: Using persistent image display"
    echo "--------------------------------------"
    # Try with the 'p' (persistent) option
    printf '\033[2J\033[H'  # Clear screen first
    kitty +kitten icat --place=50x30@1x1 '/tmp/chonker_kitty_1.jpg'
    
    echo "Testing if moving cursor affects it..."
    printf '\033[10;10H'  # Move cursor
    echo "Text here"
    printf '\033[15;15H'
    echo "More text"
    
    echo ""
    echo "Is the image still visible with text on top?"
    sleep 3
else
    echo "No test image found"
fi

echo ""
echo "=== The Problem ==="
echo "The app is clearing the screen on each render cycle,"
echo "which removes the image. We need to either:"
echo "1. Not clear the screen in A-B mode"
echo "2. Re-display the image on each render"
echo "3. Use a different approach"
