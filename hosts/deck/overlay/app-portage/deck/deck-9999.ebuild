# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

# Live ebuild: builds tools/deck from agentic-gentoo's main branch; crates are fetched in
# src_unpack from Cargo.lock. On deck, package.env points git-r3 at the local clone
# (env/deck-local-source.conf), so what is committed there is what gets built.
RUST_MIN_VER="1.88.0"

inherit cargo git-r3 systemd

DESCRIPTION="Installed packages, their origin, local patches, upstream state and usage"
HOMEPAGE="https://github.com/ivankovic/agentic-gentoo/tree/main/tools/deck"
EGIT_REPO_URI="https://github.com/ivankovic/agentic-gentoo.git"
EGIT_BRANCH="main"
S="${WORKDIR}/${P}/tools/deck"

# The repository's license; then the crates', from `cargo metadata` (where a crate offers a
# choice, one already listed: Apache-2.0 rather than BSL-1.0).
LICENSE="CC-BY-NC-ND-4.0"
LICENSE+=" Apache-2.0 MIT MPL-2.0 Unicode-3.0 Unicode-DFS-2016 Unlicense WTFPL-2 ZLIB"
SLOT="0"

# rusqlite links the system's SQLite.
DEPEND="dev-db/sqlite:3"
RDEPEND="${DEPEND}"
BDEPEND="virtual/pkgconfig"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_install() {
	local t="$(cargo_target_dir)"
	dobin "${t}/deck" "${t}/deck-focusd"
	# A root service: not in a user's PATH.
	dosbin "${t}/deck-usaged"
	systemd_dounit systemd/deck-usaged.service
	systemd_douserunit systemd/deck-focusd.service
	dodoc README.md
}

pkg_postinst() {
	elog "Usage accounting:  systemctl enable --now deck-usaged"
	elog "Active time:       systemctl --user enable --now deck-focusd   (in the graphical session)"
}
