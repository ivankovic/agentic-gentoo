# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

# Live ebuild: builds main from git. Crates are fetched in src_unpack from Cargo.lock.
inherit cargo git-r3 systemd

EGIT_REPO_URI="https://github.com/ivankovic/dom.git"
EGIT_BRANCH="main"

DESCRIPTION="Home automation: energy, network and devices, with a terminal UI"
HOMEPAGE="https://github.com/ivankovic/dom"

LICENSE="ACSL-1.4"
# The crates, from Cargo.lock.
LICENSE+=" Apache-2.0 BSD Boost-1.0 CDLA-Permissive-2.0 ISC MIT Unicode-3.0 Unlicense ZLIB"
SLOT="0"

RDEPEND="
	acct-user/dom
	app-misc/dtach
"

src_unpack() {
	git-r3_src_unpack
	# cargo.eclass runs cargo fetch where it is called from: here, the crate's directory.
	pushd "${S}" >/dev/null || die
	cargo_live_src_unpack
	popd >/dev/null || die
}

src_install() {
	cargo_src_install
	# The service runs Dom on a dtach terminal; dom-attach shows its TUI.
	systemd_dounit "${FILESDIR}"/dom.service
	dobin "${FILESDIR}"/dom-attach
	dodoc README.md
}

pkg_postinst() {
	elog "Dom keeps its state in /var/lib/dom (db.sqlite, dom.log). Start it with"
	elog "  systemctl enable --now dom"
	elog "and see its interface with dom-attach (Ctrl-\\ detaches; q stops Dom, and the service"
	elog "starts it again)."
}
