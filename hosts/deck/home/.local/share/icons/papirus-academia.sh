#!/usr/bin/env bash
# papirus-academia.sh: build ~/.local/share/icons/Papirus-Academia, Papirus-Dark with folders in
# another colour (default palebrown, which sits closest to the brass of quickshell/Theme.qml).
#
#   papirus-academia.sh [colour]     colours: the folder-<colour>.svg names in Papirus/48x48/places
#
# The system theme (x11-themes/papirus-icon-theme) stays untouched: this theme holds only symlinks
# from the plain names (folder.svg, folder-documents.svg, user-home.svg, ...) to the coloured
# icons, the same links papirus-folders makes in place, and inherits everything else. Run it again
# after a Papirus update adds icons.
set -euo pipefail

colour=${1:-palebrown}
src=/usr/share/icons
out=$HOME/.local/share/icons/Papirus-Academia

[[ -f $src/Papirus/48x48/places/folder-$colour.svg ]] ||
  { echo "papirus-academia: no folder-$colour.svg in $src/Papirus; is Papirus installed?" >&2; exit 1; }

rm -rf "$out"
mkdir -p "$out"

# Papirus-Dark last, so its icons win where it has its own.
for theme in Papirus Papirus-Dark; do
  for dir in "$src/$theme"/*/places; do
    size=$(basename "$(dirname "$dir")")
    for file in "$dir/folder-$colour"{-*,}.svg "$dir/user-$colour"{-*,}.svg; do
      [[ -f $file && ! -L $file ]] || continue
      name=$(basename "$file")
      mkdir -p "$out/$size/places"
      ln -sf "$file" "$out/$size/places/${name/-$colour/}"
    done
  done
done

# Papirus's directory list, under a new name, on top of Papirus-Dark.
sed -e 's/^Name=.*/Name=Papirus-Academia/' \
    -e "s/^Comment=.*/Comment=Papirus-Dark with $colour folders/" \
    -e 's/^Inherits=.*/Inherits=Papirus-Dark,hicolor/' \
    "$src/Papirus/index.theme" > "$out/index.theme"
grep -q '^Inherits=' "$out/index.theme" || sed -i '/^Name=/a Inherits=Papirus-Dark,hicolor' "$out/index.theme"

gtk-update-icon-cache -q -f -t "$out"
echo "papirus-academia: $(find "$out" -type l | wc -l) folder icons in $colour, in $out"
