#!/bin/bash

echo "=== Chonker95 Dark Mode Test ==="
echo ""
echo "Testing dark mode PDF image display functionality..."
echo ""

# Check if required tools are available
echo "Checking prerequisites:"
echo -n "  - ghostscript (gs): "
if command -v gs >/dev/null 2>&1; then
    echo "✓ Available"
else
    echo "✗ Missing (required for PDF rendering)"
    exit 1
fi

echo -n "  - imagemagick (convert): "
if command -v convert >/dev/null 2>&1; then
    echo "✓ Available"
else
    echo "✗ Missing (required for dark mode processing)"
    echo "    Install with: brew install imagemagick"
    exit 1
fi

echo -n "  - kitty terminal: "
if [[ -n "$KITTY_WINDOW_ID" ]] || [[ "$TERM_PROGRAM" == "kitty" ]]; then
    echo "✓ Running in kitty"
else
    echo "⚠ Not detected (image display may fall back to external viewer)"
fi

echo ""
echo "Build and test instructions:"
echo "1. Run: cargo build --release"
echo "2. Test with any PDF: ./target/release/chonker95 your_pdf.pdf"
echo "3. Press Ctrl+A to enter A-B comparison mode (shows PDF image + text)"
echo "4. Press Ctrl+D to toggle between DARK and LIGHT theme for PDF"
echo "5. The status line will show current theme mode"
echo ""
echo "Dark mode features:"
echo "- Inverts PDF colors (white background becomes black)"
echo "- Reduces brightness and saturation slightly for eye comfort"
echo "- Adds subtle blue tint for warmer dark theme"
echo "- Fallback gracefully if ImageMagick is unavailable"
echo ""

# Build the project
echo "Building chonker95 with dark mode support..."
cargo build --release

if [[ $? -eq 0 ]]; then
    echo "✓ Build successful!"
    echo ""
    echo "Usage:"
    echo "  ./target/release/chonker95 path/to/your.pdf"
    echo ""
    echo "Controls:"
    echo "  Ctrl+A  - Toggle A-B comparison mode (PDF + text)"
    echo "  Ctrl+D  - Toggle dark/light theme for PDF"
    echo "  Q       - Quit"
else
    echo "✗ Build failed"
    exit 1
fi