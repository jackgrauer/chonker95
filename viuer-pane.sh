#!/bin/bash

# viuer-pane script for chonker95 pane 2
# Handles PDF display with viuer and sync communication

PDF_FILE="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CURRENT_PAGE=1

if [ -z "$PDF_FILE" ]; then
    echo "Error: No PDF file specified"
    echo "Usage: $0 <pdf-file>"
    exit 1
fi

# Create socket for sync communication
SOCKET_NAME="chonker95_$(basename "$PDF_FILE" .pdf).sock"
SOCKET_PATH="/tmp/$SOCKET_NAME"

# Function to display PDF page with viuer
display_pdf_page() {
    local page="$1"
    clear

    # Convert PDF page to image
    local temp_img="/tmp/chonker95_page_${page}.png"

    # Use our pdf2img converter
    if DYLD_LIBRARY_PATH="$SCRIPT_DIR/lib" "$SCRIPT_DIR/target/release/pdf2img" "$PDF_FILE" "$page" "$temp_img" 2>/dev/null; then
        # Display with viuer
        if "$SCRIPT_DIR/target/release/viuer-display" "$temp_img" 2>/dev/null; then
            echo ""
            echo "  PDF Page: $page"
            echo "  Ctrl+P: toggle | Ctrl+←→: pages"
        else
            echo ""
            echo "  PDF Page: $page"
            echo "  [Image display failed]"
            echo "  Ctrl+P: toggle panes"
        fi
    else
        echo ""
        echo "  PDF Page: $page"
        echo "  [PDF conversion failed]"
        echo "  Ctrl+P: toggle panes"
    fi
}

# Listen for sync messages from chonker95
listen_for_sync() {
    if [ -e "$SOCKET_PATH" ]; then
        # Listen to socket for page changes
        while read -r line; do
            if [[ "$line" == *"PageChange"* ]]; then
                # Extract page number from JSON
                local new_page=$(echo "$line" | grep -o '"PageChange":\([0-9]*\)' | grep -o '[0-9]*')
                if [ -n "$new_page" ]; then
                    CURRENT_PAGE="$new_page"
                    display_pdf_page "$CURRENT_PAGE"
                fi
            fi
        done < <(nc -U -l "$SOCKET_PATH" 2>/dev/null || true)
    fi
}

# Handle keyboard input
handle_input() {
    while true; do
        read -n 1 -s key
        case "$key" in
            $'\x10')  # Ctrl+P
                zellij action close-pane
                exit 0
                ;;
            $'\x1b')  # Escape sequence (arrow keys)
                read -n 2 -s arrow
                case "$arrow" in
                    '[C')  # Right arrow / Ctrl+Right
                        CURRENT_PAGE=$((CURRENT_PAGE + 1))
                        display_pdf_page "$CURRENT_PAGE"
                        ;;
                    '[D')  # Left arrow / Ctrl+Left
                        if [ "$CURRENT_PAGE" -gt 1 ]; then
                            CURRENT_PAGE=$((CURRENT_PAGE - 1))
                            display_pdf_page "$CURRENT_PAGE"
                        fi
                        ;;
                esac
                ;;
        esac
    done
}

# Start everything
display_pdf_page "$CURRENT_PAGE"

# Start background sync listener
listen_for_sync &

# Handle input in foreground
handle_input