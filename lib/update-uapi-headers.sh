#!/bin/bash
# Copy V4L2-related kernel UAPI headers from the host system into the v4l2r
# library so that builds against older cross-toolchain sysroots still compile.
#
# The copied headers are placed under lib/uapi/linux/ and are used by build.rs
# when the "builtin-uapi-header" feature is enabled, or when the target
# sysroot's kernel headers are detected to be too old.
#
# Usage:
#   ./update-uapi-headers.sh [SOURCE_DIR]
#
# SOURCE_DIR defaults to /usr/include.  It must contain linux/videodev2.h.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC="${1:-/usr/include}"
DEST="${SCRIPT_DIR}/uapi"

# The set of v4l2-specific UAPI headers to copy.
# System headers they depend on (linux/types.h, linux/ioctl.h, linux/const.h,
# sys/time.h) are intentionally NOT copied – they come from the toolchain.
HEADERS=(
    linux/videodev2.h
    linux/v4l2-common.h
    linux/v4l2-controls.h
)

echo "Source: ${SRC}"
echo "Destination: ${DEST}"

# Verify source headers exist
for h in "${HEADERS[@]}"; do
    if [ ! -f "${SRC}/${h}" ]; then
        echo "ERROR: ${SRC}/${h} not found" >&2
        exit 1
    fi
done

# Record the source kernel version for reference
VERSION_H="${SRC}/linux/version.h"
if [ -f "${VERSION_H}" ]; then
    VERSION_CODE=$(grep -m1 'LINUX_VERSION_CODE' "${VERSION_H}" | awk '{print $3}')
    MAJOR=$(( VERSION_CODE >> 16 ))
    MINOR=$(( (VERSION_CODE >> 8) & 0xFF ))
    PATCH=$(( VERSION_CODE & 0xFF ))
    echo "Source kernel header version: ${MAJOR}.${MINOR}.${PATCH} (${VERSION_CODE})"
fi

# Clean and recreate
rm -rf "${DEST}"
mkdir -p "${DEST}/linux"

# Copy headers
for h in "${HEADERS[@]}"; do
    cp "${SRC}/${h}" "${DEST}/${h}"
    echo "Copied ${h}"
done

# Record provenance
cat > "${DEST}/README" <<EOF
These kernel UAPI headers were copied from the host system by update-uapi-headers.sh.
Source: ${SRC}
Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)
Kernel version: ${MAJOR:-unknown}.${MINOR:-unknown}.${PATCH:-unknown}

They are used when the cross-toolchain's built-in headers are too old to
contain the V4L2 data structures needed by v4l2r.
EOF

echo "Done. Headers are in ${DEST}/"
