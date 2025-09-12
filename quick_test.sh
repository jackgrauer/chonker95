#!/bin/bash

echo "=== Quick Fix Test ==="
echo ""

cd /Users/jack/chonker95

# Rebuild with the fixes
echo "Rebuilding with screen clear fix..."
cargo build --release 2>&1 | tail -n 2

echo ""
echo "Testing the app now..."
echo ""
echo "Instructions:"
echo "1. Press Ctrl+A to enable A-B mode"
echo "2. The image should appear and STAY in the left panel"  
echo "3. Press Q to quit"
echo ""

# Create a test PDF
TEST_PDF="/tmp/test.pdf"
echo "Test PDF for Chonker95" | ps2pdf - "$TEST_PDF" 2>/dev/null || TEST_PDF=""

if [ -n "$TEST_PDF" ]; then
    echo "Starting app with test PDF..."
    ./target/release/chonker95 "$TEST_PDF"
else
    echo "Run: ./target/release/chonker95 your_pdf.pdf"
fi

echo ""
echo "Checking debug log..."
cat /tmp/chonker_debug.log 2>/dev/null || echo "No debug log"
