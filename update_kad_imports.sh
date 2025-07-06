#!/bin/bash

# Script to update libp2p::kad imports to ant_kad

echo "Updating kad imports across the codebase..."

# Find all Rust files and update the imports
find . -name "*.rs" -not -path "./target/*" -not -path "./ant-kad/*" | while read -r file; do
    # Check if file contains libp2p::kad
    if grep -q "libp2p::kad" "$file"; then
        echo "Updating $file"
        
        # Update basic imports
        sed -i.bak 's/use libp2p::kad::/use ant_kad::/g' "$file"
        sed -i.bak 's/libp2p::kad::/ant_kad::/g' "$file"
        
        # Handle specific patterns for kad::
        sed -i.bak 's/kad::/ant_kad::/g' "$file" 
        
        # Clean up backup files
        rm -f "$file.bak"
    fi
done

echo "Import updates completed!"