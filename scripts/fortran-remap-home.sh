#!/usr/bin/env bash
# fortran-remap-home.sh <compiler> <args...> — CMake's Fortran compiler launcher in the installer
# builds (build-windows-cross.sh and build-linux.sh set CMAKE_Fortran_COMPILER_LAUNCHER to it).
#
# gfortran embeds the path of every source file it compiles, for its run-time and I/O error
# messages ("At line 7 of file …"), and no -ffile-prefix-map reaches those strings: measured on
# GCC 13, they, the preprocessor's line markers and __FILE__ all keep the full path. It embeds the
# path exactly as it was given, so keep $HOME out of what it is given, for both generators:
#   • Ninja (the cross build) preprocesses each source into `<source>-pp.f90` first and compiles
#     that, taking the path from its line markers (`# 1 "/home/<user>/…"`). Rewrite $HOME at the
#     start of each marker to `~`, the mapping rustc and gcc get from the same scripts. CMake's
#     dependency scan read the markers when it preprocessed, so dependency tracking is unchanged.
#   • Unix Makefiles (the native Linux build) compile the source itself, named by its absolute
#     path. Name it relative to the working directory instead; it is the same file.
# Every other argument passes untouched.
set -euo pipefail
home_re=$(printf '%s' "$HOME" | sed 's/[]\\.*^$|[]/\\&/g')
args=("$@")
for i in "${!args[@]}"; do
  a=${args[i]}
  case "$a" in
    *-pp.*) if [ -f "$a" ]; then sed -i "s|^\(# [0-9][0-9]* \"\)$home_re/|\1~/|" "$a"; fi ;;
    "$HOME"/*.[fF]|"$HOME"/*.[fF]90) if [ -f "$a" ]; then args[i]=$(realpath --relative-to=. -- "$a"); fi ;;
  esac
done
exec "${args[@]}"
