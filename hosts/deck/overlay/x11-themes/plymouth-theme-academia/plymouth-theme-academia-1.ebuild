# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

DESCRIPTION="Dark academia Plymouth theme: a brass astrolabe on espresso"
HOMEPAGE="https://github.com/ivankovic/agentic-gentoo"
S="${WORKDIR}"

LICENSE="GPL-2"
SLOT="0"
KEYWORDS="~amd64"

RDEPEND="sys-boot/plymouth"

src_install() {
	insinto /usr/share/plymouth/themes/academia
	# The images are pre-rendered by make-images.py (kept beside them, not installed).
	doins "${FILESDIR}"/academia/{academia.plymouth,academia.script,*.png}
}
