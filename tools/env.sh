# Sourced by tools/compare.sh. Locates the Java reference build and, on a machine
# without a GPU or display, selects Mesa's llvmpipe on a virtual X display.
#
#   M3G_JAVA_REF   directory of the patched Java viewer (default: ../ref_java)
#   M3G_FRAMES     where rendered frames go            (default: target/frames)
#   M3G_LLVMPIPE=1 force Mesa software rendering (LIBGL_ALWAYS_SOFTWARE, Mesa GLX/EGL)
#   DISPLAY        must point at an X server; use `Xvfb :99 -screen 0 1280x1024x24 +extension GLX`
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export M3G_JAVA_REF="${M3G_JAVA_REF:-$REPO/../ref_java}"
export M3G_FRAMES="${M3G_FRAMES:-$REPO/target/frames}"
if [ "${M3G_LLVMPIPE:-0}" = "1" ]; then
  export LIBGL_ALWAYS_SOFTWARE=1
  export __GLX_VENDOR_LIBRARY_NAME=mesa
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
  export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/xdg-$(id -u)}"
  mkdir -p "$XDG_RUNTIME_DIR"
fi
