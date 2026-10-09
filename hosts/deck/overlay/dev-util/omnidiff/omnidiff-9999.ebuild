# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU Affero General Public License v3 or later

EAPI=8

# Live ebuild: builds main from git. Crates are fetched in src_unpack from Cargo.lock.
# Install with  emerge --oneshot =dev-util/omnidiff-9999  (needs ** in package.accept_keywords).
# Cargo.toml's rust-version (the release ebuild still says 1.88.0).
RUST_MIN_VER="1.92.0"

inherit bash-completion-r1 cargo git-r3

EGIT_REPO_URI="https://github.com/ivankovic/omnidiff.git"
EGIT_BRANCH="main"

DESCRIPTION="Fast, robust, accurate content-aware diffing"
HOMEPAGE="https://github.com/ivankovic/omnidiff"

# Same license list as the release ebuild; the crates come from Cargo.lock at build time.
LICENSE="AGPL-3+"
LICENSE+=" Apache-2.0 BSD BSD-2 BZIP2 ISC MIT MPL-2.0 Unicode-DFS-2016 Unlicense ZLIB"
SLOT="0"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

# Every tree-sitter grammar is compiled from C at build time, so a C compiler is required. It is
# part of @system on Gentoo, hence no explicit DEPEND - but it is why this package is not a pure
# Rust build and why the build takes noticeably longer than the crate count alone suggests.

# Built with the default `tui` feature only - the product binary. The `stats` and `test-fixtures`
# features gate dataset-analysis dev tools that need git2 (OpenSSL, libssh2) and a bundled SQLite;
# they are not part of the shipped product, so there is no USE flag for them.

# `lto = "fat"` plus `codegen-units = 1` in the release profile (see Cargo.toml) makes this a slow
# single-threaded link. That is deliberate upstream - omnidiff is CPU-bound at run time - but it is
# worth knowing before reporting the build as hung.

src_test() {
	# The GitHub tag tarball carries tests/ and the fixture corpus (the crates.io tarball excludes
	# both), so the suite is runnable here - but the corpus tests are the accuracy benchmark, which
	# needs the `test-fixtures` feature and a large amount of time and memory. Restrict to the
	# library's own unit tests, which are what a packaging sanity check actually wants.
	cargo_src_test --lib
}

src_install() {
	cargo_src_install

	# Generated from the same clap definition as --help, by the binary that was just built. Native
	# build only; if this package ever grows a cross-compile path, these have to move to a
	# host-built artifact instead.
	# Hardcoded rather than $(usex debug ...): that idiom needs a `debug` USE flag in IUSE, and
	# this package deliberately offers no debug build - the release profile's lto/codegen-units
	# settings are the point of it (see Cargo.toml).
	local omnidiff="${S}/target/release/omnidiff"
	"${omnidiff}" util man > "${T}/${PN}.1" || die "failed to generate man page"
	doman "${T}/${PN}.1"

	"${omnidiff}" util completions bash > "${T}/${PN}.bash" || die
	newbashcomp "${T}/${PN}.bash" "${PN}"

	"${omnidiff}" util completions zsh > "${T}/_${PN}" || die
	insinto /usr/share/zsh/site-functions
	doins "${T}/_${PN}"

	"${omnidiff}" util completions fish > "${T}/${PN}.fish" || die
	insinto /usr/share/fish/vendor_completions.d
	doins "${T}/${PN}.fish"

	dodoc README.md CONTRIBUTING.md
}

pkg_postinst() {
	elog "Configure omnidiff as git's diff tool with:  omnidiff git configure"
	elog "Configure omnidiff as jj's diff tool with:   omnidiff jj configure"
}
