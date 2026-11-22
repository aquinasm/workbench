#!/usr/bin/env sh
set -eu

REPO="aquinasm/workbench"
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "==> Detecting latest Workbench release from github.com/${REPO}..."
TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=1" | grep -m1 '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"

if [ -z "$TAG" ]; then
  echo "Error: Could not determine latest release tag." >&2
  exit 1
fi

if [ "$OS" = "Darwin" ]; then
  if [ "$ARCH" != "arm64" ] && [ "$ARCH" != "aarch64" ]; then
    echo "Error: Workbench on macOS requires Apple Silicon (arm64)." >&2
    exit 1
  fi
  ASSET="Workbench_aarch64.app.tar.gz"
  URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT

  echo "==> Downloading Workbench ${TAG} (${ARCH})..."
  curl -fL "$URL" -o "${TMP_DIR}/${ASSET}"

  echo "==> Extracting Workbench.app to /Applications..."
  tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
  rm -rf /Applications/Workbench.app
  mv "${TMP_DIR}/Workbench.app" /Applications/Workbench.app
  xattr -cr /Applications/Workbench.app 2>/dev/null || true
  codesign --force --deep --sign - /Applications/Workbench.app 2>/dev/null || true

  echo "==> Installed /Applications/Workbench.app (${TAG})"
else
  echo "For Linux or Windows, download the installer from:"
  echo "  https://github.com/${REPO}/releases/tag/${TAG}"
fi
