#!/bin/bash

echo "=== Debugging Chonker95 Image Display ==="
echo ""

cd /Users/jack/chonker95

# 1. Check when the binary was last built
echo "1. Binary build time:"
ls -la target/release/chonker95
echo ""

# 2. Check the source code has the new method
echo "2. Checking source code for 'kitty +kitten icat' command:"
grep -n "kitty +kitten icat" src/main.rs | head -n 2
echo ""

# 3. Force a clean rebuild
echo "3. Force rebuilding to ensure new code is compiled..."
touch src/main.rs  # Touch the file to force recompilation
cargo build --release 2>&1 | tail -n 3
echo ""

# 4. Create a test PDF if needed
TEST_PDF="/tmp/test_chonker.pdf"
if command -v ps2pdf >/dev/null 2>&1; then
    echo "Test Document for Chonker95" | ps2pdf - "$TEST_PDF"
    echo "Created test PDF: $TEST_PDF"
else
    TEST_PDF=""
    echo "No ps2pdf available"
fi
echo ""

# 5. Run the app and check debug log
echo "4. Running the app..."
echo "   Instructions:"
echo "   - Press Ctrl+A to enable A-B mode"
echo "   - Wait 2 seconds"
echo "   - Press Q to quit"
echo ""

if [ -n "$TEST_PDF" ]; then
    # Run in background, wait, then kill
    timeout 10 ./target/release/chonker95 "$TEST_PDF" &
    APP_PID=$!
    
    echo "App started with PID $APP_PID"
    echo "Waiting for you to press Ctrl+A..."
    sleep 5
    
    echo ""
    echo "5. Checking debug log after run:"
    if [ -f /tmp/chonker_debug.log ]; then
        echo "=== Debug Log Contents ==="
        cat /tmp/chonker_debug.log
        echo "==========================="
        echo ""
        
        # Check if it's using the new or old method
        if grep -q "Success: Image displayed via icat" /tmp/chonker_debug.log; then
            echo "✅ NEW CODE IS RUNNING (using icat method)"
        elif grep -q "DEBUG: Sent graphics protocol" /tmp/chonker_debug.log; then
            echo "❌ OLD CODE IS STILL RUNNING"
            echo "The binary wasn't updated. Let's fix this..."
            echo ""
            echo "Cleaning and rebuilding..."
            cargo clean
            cargo build --release
            echo "Try running again: ./target/release/chonker95 $TEST_PDF"
        else
            echo "⚠️  Unexpected debug log format"
        fi
    else
        echo "No debug log found"
    fi
else
    echo "Please run manually: ./target/release/chonker95 your_pdf.pdf"
fi

echo ""
echo "6. Let's test the icat command directly with the generated image:"
if [ -f /tmp/chonker_kitty_1.jpg ]; then
    echo "Testing direct icat command..."
    kitty +kitten icat --clear --place=50x30@1x1 --scale-up /tmp/chonker_kitty_1.jpg
    echo ""
    echo "Did you see the image above? That's what should appear in the app."
    sleep 2
    kitty +kitten icat --clear
fi
