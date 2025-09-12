#!/bin/bash

echo "=== Kitty Terminal Environment Check ==="
echo ""
echo "Kitty-specific environment variables:"
echo "  KITTY_WINDOW_ID: ${KITTY_WINDOW_ID:-not set}"
echo "  KITTY_PID: ${KITTY_PID:-not set}"
echo "  KITTY_PUBLIC_KEY: ${KITTY_PUBLIC_KEY:+[set]}"
echo "  KITTY_INSTALLATION_DIR: ${KITTY_INSTALLATION_DIR:-not set}"
echo ""
echo "General terminal environment:"
echo "  TERM: $TERM"
echo "  TERM_PROGRAM: ${TERM_PROGRAM:-not set}"
echo "  COLORTERM: ${COLORTERM:-not set}"
echo ""

if [ -n "$KITTY_WINDOW_ID" ]; then
    echo "✅ You are definitely in Kitty terminal (Window ID: $KITTY_WINDOW_ID)"
    
    # Check Kitty version and capabilities
    if command -v kitty >/dev/null 2>&1; then
        echo ""
        echo "Kitty version:"
        kitty --version
        
        echo ""
        echo "Testing graphics protocol..."
        
        # Create a simple test image
        TEST_IMG="/tmp/kitty_env_test.jpg"
        if command -v convert >/dev/null 2>&1; then
            convert -size 200x100 gradient:blue-cyan \
                    -pointsize 20 -fill white -gravity center \
                    -annotate +0+0 "Window ID: $KITTY_WINDOW_ID" \
                    "$TEST_IMG" 2>/dev/null
        else
            # Create a test image using base64 if ImageMagick isn't available
            # This is a tiny 1x1 red pixel JPEG
            echo "/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAr/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCwAA8A/9k=" | base64 -d > "$TEST_IMG"
        fi
        
        if [ -f "$TEST_IMG" ]; then
            echo "Displaying test image using direct protocol..."
            printf '\e_Gt=f,a=T,c=20,r=10;%s\e\\' "$TEST_IMG"
            echo ""
            echo ""
            echo "If you see an image above, graphics protocol is working!"
        fi
    else
        echo "⚠️  kitty command not found in PATH"
    fi
else
    echo "❌ Not in Kitty terminal or KITTY_WINDOW_ID not set"
    echo ""
    echo "Possible reasons:"
    echo "1. You're not using Kitty terminal"
    echo "2. You're using an older version of Kitty"
    echo "3. You're in a tmux/screen session that doesn't pass through env vars"
    echo ""
    echo "To fix tmux/screen issues, try:"
    echo "  export KITTY_WINDOW_ID=$KITTY_WINDOW_ID"
fi

echo ""
echo "=== Quick Image Test ==="
echo "Creating and displaying a test image..."

# Create test image
TEST_FILE="/tmp/quick_kitty_test.jpg"
if command -v convert >/dev/null 2>&1; then
    convert -size 300x150 plasma:fractal "$TEST_FILE" 2>/dev/null
    echo "Test image created: $TEST_FILE"
else
    # Fallback: create a simple JPEG using echo and base64
    echo "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/2wBDAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/wAARCAABAAEDASIAAhEBAxEB/8QAFQABAQAAAAAAAAAAAAAAAAAAAAv/xAAUEAEAAAAAAAAAAAAAAAAAAAAA/8QAFQEBAQAAAAAAAAAAAAAAAAAAAAX/xAAUEQEAAAAAAAAAAAAAAAAAAAAA/9oADAMBAAIRAxEAPwCwAA8A/9k=" | base64 -d > "$TEST_FILE"
    echo "Minimal test image created: $TEST_FILE"
fi

if [ -f "$TEST_FILE" ]; then
    echo ""
    echo "Method 1: Simple file reference"
    printf '\e_Gt=f;%s\e\\' "$TEST_FILE"
    sleep 0.5
    
    echo ""
    echo "Method 2: With explicit action=transmit"
    printf '\e_Ga=T,t=f;%s\e\\' "$TEST_FILE"
    sleep 0.5
    
    echo ""
    echo "Method 3: With size (20 cols)"
    printf '\e_Ga=T,t=f,c=20;%s\e\\' "$TEST_FILE"
    
    echo ""
    echo ""
    echo "Check complete!"
fi
