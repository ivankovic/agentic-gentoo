# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU General Public License v2

EAPI=8

# Live ebuild: installs tools/fix from agentic-gentoo's main branch. On deck, package.env points
# git-r3 at the local clone (env/deck-local-source.conf), so what is committed there is installed.
PYTHON_COMPAT=( python3_{12..14} )

inherit git-r3 python-single-r1 systemd

DESCRIPTION="An offline sysadmin model's tools: fix-shadow compares it with the main agent"
HOMEPAGE="https://github.com/ivankovic/agentic-gentoo/tree/main/tools/fix"
EGIT_REPO_URI="https://github.com/ivankovic/agentic-gentoo.git"
EGIT_BRANCH="main"
S="${WORKDIR}/${P}/tools/fix"

LICENSE="CC-BY-NC-ND-4.0"
SLOT="0"
REQUIRED_USE="${PYTHON_REQUIRED_USE}"

# llama-server runs the model (--serve).
RDEPEND="
	${PYTHON_DEPS}
	sci-misc/llama-cpp
"

src_install() {
	python_doscript fix-shadow
	# What it shares with the drills' runner (tools/drills): clip and the chat request.
	python_domodule ../drills/modelchat.py
	systemd_douserunit systemd/fix-shadow.service systemd/fix-shadow.timer
	dodoc README.md
}

pkg_postinst() {
	elog "Nightly comparison with the main agent:  systemctl --user enable --now fix-shadow.timer"
	elog "The model: FIX_MODEL in a drop-in for fix-shadow.service (default ~/models/Qwen3.5-2B-Q5_K_M.gguf)"
}
