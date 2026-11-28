#!/bin/sh
# Stands in for the KiCadRoutingTools Python. The sibling `stub-mode` file
# decides what the current attempt does, so the test never rewrites an
# executable while another test may be forking.
if [ "${1:-}" = "-version" ]; then
    echo "Python 3.12.3"
    exit 0
fi

mode_file="$(dirname "${2:-.}")/stub-mode"
mode="$(cat "$mode_file" 2>/dev/null || echo first)"
case "$mode" in
    second)
        echo "second run output"
        exec sleep 60
        ;;
    silent)
        exec sleep 60
        ;;
    *)
        echo "first run output"
        ;;
esac
