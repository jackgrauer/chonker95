# Chonker95 Image Display Fix

## Summary of Issues Found

Your terminal-based PDF viewer wasn't displaying images due to several issues with the Kitty graphics protocol implementation:

### 1. **Incorrect Escape Sequence Format**
- **Original**: `\x1b_Gf=100,t=f,a=T,c={};{}\x1b\`
- **Fixed**: `\x1b_Ga=T,t=t,f=100,c={},r={};{}\x1b\\`
- The terminator should be `\x1b\\` (with double backslash), not `\x1b\`

### 2. **Raw Terminal Mode Interference**
- The Kitty graphics protocol doesn't work properly when the terminal is in raw mode
- **Solution**: Temporarily disable raw mode before sending graphics commands, then re-enable it

### 3. **Missing Image Dimensions**
- Original code only specified columns (`c`) but not rows (`r`)
- **Solution**: Added both `c` (columns) and `r` (rows) parameters

### 4. **File Path Encoding**
- The protocol supports both base64-encoded paths and direct file paths
- **Solution**: Implemented both methods for better compatibility

## How to Test the Fix

1. **Rebuild the application**:
   ```bash
   cd /Users/jack/chonker95
   cargo build --release
   ```

2. **Test with a PDF file**:
   ```bash
   ./target/release/chonker95 your_pdf_file.pdf
   ```

3. **Toggle A-B comparison mode**:
   - Press `Ctrl+A` to toggle between text-only and A-B comparison mode
   - In A-B mode, the left panel should show the PDF page as an image

4. **Run the test script**:
   ```bash
   chmod +x test_kitty_graphics.sh
   ./test_kitty_graphics.sh
   ```

## Troubleshooting

If images still don't display:

### 1. **Verify you're using Kitty terminal**:
```bash
echo $TERM_PROGRAM  # Should output "kitty"
echo $TERM          # Should contain "kitty"
```

### 2. **Check Kitty configuration**:
Ensure graphics are enabled in `~/.config/kitty/kitty.conf`:
```
allow_remote_control yes
```

### 3. **Test Kitty graphics directly**:
```bash
# Create a test image
convert -size 200x100 xc:red test.jpg

# Display it using kitty icat
kitty +kitten icat test.jpg

# Or using the graphics protocol directly
printf '\e_Gt=f,a=T;test.jpg\e\\'
```

### 4. **Check debug logs**:
```bash
cat /tmp/chonker_debug.log
```

### 5. **Verify Ghostscript is working**:
```bash
# Test PDF to JPG conversion
gs -dNOPAUSE -dBATCH -dSAFER -dQUIET \
   -sDEVICE=jpeggray -dFirstPage=1 -dLastPage=1 \
   -r150 -dJPEGQ=85 -sOutputFile=/tmp/test_page.jpg \
   your_pdf_file.pdf

# Check if the image was created
ls -la /tmp/test_page.jpg
```

## Alternative Solutions

If Kitty graphics still don't work:

### 1. **Use a different terminal with image support**:
- iTerm2 (macOS) with inline images protocol
- WezTerm with its image protocol
- Sixel-capable terminals (xterm with sixel support)

### 2. **Modify the fallback to use a different viewer**:
The code already has a fallback that opens the image in Preview.app on macOS. You can modify this to use any image viewer you prefer.

### 3. **Use ASCII art rendering**:
Consider adding an ASCII art renderer as a fallback for terminals without image support.

## Key Code Changes Made

1. Added `base64::Engine` import for encoding file paths
2. Modified `display_image_in_kitty()` function to:
   - Temporarily disable/re-enable raw mode
   - Use correct escape sequence format
   - Try both base64-encoded and direct file path methods
   - Add proper row and column parameters
   - Clear the display area before rendering

## Additional Notes

- The application uses pdfalto for text extraction (make sure it's installed)
- Images are rendered as grayscale JPEGs at 150 DPI for better terminal display
- The debug log at `/tmp/chonker_debug.log` can help diagnose issues
- Press `Q` to quit the application
