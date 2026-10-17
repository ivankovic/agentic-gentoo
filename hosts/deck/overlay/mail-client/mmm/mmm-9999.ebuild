# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

# Live ebuild: builds tools/mmm from agentic-gentoo's main branch; crates are fetched in
# src_unpack from Cargo.lock. On deck, package.env points git-r3 at the local clone
# (env/deck-local-source.conf), so what is committed there is what gets built.
RUST_MIN_VER="1.88.0"

inherit cargo git-r3

DESCRIPTION="Marko's Mail Machine: a terminal mail reader over notmuch"
HOMEPAGE="https://github.com/ivankovic/agentic-gentoo/tree/main/tools/mmm"
EGIT_REPO_URI="https://github.com/ivankovic/agentic-gentoo.git"
EGIT_BRANCH="main"
S="${WORKDIR}/${P}/tools/mmm"

# The repository's license; then the crates', from `cargo metadata` (where a crate offers a
# choice, one already listed).
LICENSE="CC-BY-NC-ND-4.0"
LICENSE+=" 0BSD Apache-2.0 BSD MIT Unicode-3.0 Unicode-DFS-2016 WTFPL-2 ZLIB"
SLOT="0"

# notmuch reads and tags the mail; xdg-open opens links and attachments; wl-copy copies.
RDEPEND="
	net-mail/notmuch
	x11-misc/xdg-utils
	gui-apps/wl-clipboard
	sys-apps/systemd
"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_install() {
	dobin "$(cargo_target_dir)/mmm"
	dodoc README.md
}
