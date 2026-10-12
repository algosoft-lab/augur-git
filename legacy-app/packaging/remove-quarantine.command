#!/bin/bash
# Augur Git - remove macOS quarantine attributes from the installed app.
# Run this after moving Augur Git.app into the Applications folder.

APP_PATH="/Applications/Augur Git.app"

if [ ! -d "$APP_PATH" ]; then
    echo "Application not found at $APP_PATH."
    echo "Move Augur Git.app into the Applications folder, then run this command again."
    echo
    read -r -p "Press Enter to exit..."
    exit 1
fi

echo "Removing macOS quarantine attributes. Administrator permission may be required..."
echo

if sudo xattr -rd com.apple.quarantine "$APP_PATH"; then
    echo
    echo "Done. Augur Git can now be opened from Applications."
    echo "If macOS still blocks it, use System Settings > Privacy & Security > Open Anyway."
else
    echo
    echo "The operation failed. Check the administrator password and try again."
fi

echo
read -r -p "Press Enter to exit..."
