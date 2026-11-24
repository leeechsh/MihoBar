#!/usr/bin/env bash
# Downloads the mihomo core (MetaCubeX/mihomo) that Mihobar ships as
# assets/bin/mihomo.gz, as ClashBar's Scripts/preprocess.sh does.
#   MIHOMO_VERSION=v1.19.x  pin a release (default: the latest)
#   TARGET_ARCH=arm64|x86_64 (default: this Mac's)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="${MIHOMO_REPO:-MetaCubeX/mihomo}"
VERSION="${MIHOMO_VERSION:-}"
ARCH="${TARGET_ARCH:-$(uname -m)}"
OUT="$ROOT/assets/bin/mihomo.gz"

case "$ARCH" in
  x86_64 | amd64) CANDIDATES=(amd64-v2-go122 amd64) ;;
  arm64 | aarch64) CANDIDATES=(arm64-go122 arm64) ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

if [ -z "$VERSION" ]; then
  VERSION="$(curl -fsSL "https://github.com/$REPO/releases/latest/download/version.txt" | tr -d '\r\n')"
fi
[[ "$VERSION" == v* ]] || VERSION="v$VERSION"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
for flavor in "${CANDIDATES[@]}"; do
  asset="mihomo-darwin-$flavor-$VERSION.gz"
  url="https://github.com/$REPO/releases/download/$VERSION/$asset"
  if curl -fsSL "$url" -o "$TMP/mihomo.gz"; then
    gzip -t "$TMP/mihomo.gz"
    gunzip -c "$TMP/mihomo.gz" > "$TMP/mihomo"
    file "$TMP/mihomo" | grep -q 'Mach-O' || { echo "$asset is not a Mach-O binary" >&2; exit 1; }
    mv "$TMP/mihomo.gz" "$OUT"
    echo "$VERSION" > "$ROOT/assets/bin/mihomo.version"
    echo "fetched $asset -> $OUT"
    exit 0
  fi
done
echo "no mihomo asset found for $ARCH $VERSION" >&2
exit 1
