#!/usr/bin/env bash
# Copy the built game into the GitHub Pages site folder (stage 8, deploy).
# It only assembles files; publishing (git commit and push in the site
# folder) is the person's decision.
#
#   ./scripts/pages.sh                    → ../ptonkle.github.io/mandible/
#   ./scripts/pages.sh /path/to/site      → /path/to/site/mandible/
set -euo pipefail
cd "$(dirname "$0")/.."
SITE="${1:-../ptonkle.github.io}"
./scripts/build.sh >/dev/null
rm -rf "$SITE/mandible"
mkdir -p "$SITE/mandible"
cp -r dist/. "$SITE/mandible/"
# The transport test page is for checks only.
rm -f "$SITE/mandible/nettest.html" "$SITE/mandible/nettest.js"
touch "$SITE/.nojekyll"
echo "Assembled $SITE/mandible/:"
find "$SITE/mandible" -type f | sort
