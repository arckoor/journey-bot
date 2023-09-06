#!/usr/bin/bash
if [ -e upgradeRequest ]; then
    git pull origin
    source ../venv/bin/activate
    python3 -m pip install -U -r requirements.txt
    rm -rf upgradeRequest
fi
