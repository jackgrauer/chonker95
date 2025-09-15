#!/bin/bash

# chonker95 Zellij launcher script
# Usage: ./chonker95.sh document.pdf

PDF_FILE="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -z "$PDF_FILE" ]; then
    echo "Usage: $0 <pdf-file>"
    exit 1
fi

# Check if already in Zellij
if [ -n "$ZELLIJ" ]; then
    echo "Already in Zellij. Run chonker95 directly:"
    echo "DYLD_LIBRARY_PATH=$SCRIPT_DIR/lib $SCRIPT_DIR/target/release/chonker95 \"$PDF_FILE\""
    exit 0
fi

# Start new Zellij session with simple layout
export DYLD_LIBRARY_PATH="$SCRIPT_DIR/lib"
export CHONKER_PDF="$PDF_FILE"

# Launch Zellij with config and layout to hide borders
exec zellij --config "$SCRIPT_DIR/config.kdl" --layout "$SCRIPT_DIR/chonker95.kdl"