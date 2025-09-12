#!/bin/bash
cd /Users/jack/chonker95
echo "Building Chonker95..."
cargo build --release 2>&1 | head -n 20
echo ""
echo "Build complete. Checking for warnings..."
cargo build --release 2>&1 | grep -E "warning|error" || echo "✅ No warnings or errors!"
