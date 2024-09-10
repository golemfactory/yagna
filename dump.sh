#!/bin/bash

DIRECTORY="./"

result=$(find "$DIRECTORY" \
    \( -type d \( -name "yagna-builds" -o -name ".git" -o -name ".github" -o -name "node_modules" -o -name "test" -o -name "tests" \) -prune \) -o \
    \( -path "$DIRECTORY/static" -prune \) -o \
    \( -type f ! -name "*.pyc" ! -name "*.log" ! -name ".DS_Store" ! -iname "*license*" ! -name "celerybeat-schedule" \
    ! -iname "*.jpg" ! -iname "*.png" ! -iname "*.gif" \
    ! -iname "*.toml" ! -iname "*.jpeg" ! -iname "*.svg" ! -iname "*.bmp" \
    ! -iname "*.base64" ! -iname "*.key" ! -iname "*.pem" ! -iname "*.pub" -print \))

output=""
while IFS= read -r path; do
    if [ -d "$path" ]; then
        output+="Directory: $path\n"
    elif [ -f "$path" ]; then
        output+="File: $path\n$(cat "$path")\n"
    fi
    output+="----------------\n"
done <<<"$result"

echo -e "$output" | pbcopy

echo "Directory and file structure, including content (with specific exclusions), has been copied to your clipboard."
