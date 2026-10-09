# Copyright 2026 Marko Ivankovic
# Distributed under the terms of the GNU Affero General Public License v3 or later

EAPI=8

CRATES="
	adler2@2.0.1
	aho-corasick@1.1.5
	alloc-no-stdlib@2.0.4
	alloc-stdlib@0.2.4
	allocator-api2@0.2.21
	anstream@1.0.0
	anstyle-parse@1.0.0
	anstyle-query@1.1.5
	anstyle-wincon@3.0.11
	anstyle@1.0.14
	anyhow@1.0.104
	approx@0.5.1
	arrayref@0.3.9
	arrayvec@0.7.8
	atomic@0.6.1
	autocfg@1.5.1
	base64-simd@0.8.0
	base64@0.22.1
	base64@0.23.1
	bincode@1.3.3
	bit-set@0.5.3
	bit-set@0.8.0
	bit-vec@0.6.3
	bit-vec@0.8.0
	bitflags@1.3.2
	bitflags@2.13.2
	bitvec@1.1.1
	block-buffer@0.10.4
	block-buffer@0.12.1
	brotli-decompressor@5.0.3
	brotli@8.0.4
	bumpalo@3.20.3
	by_address@1.2.1
	bytemuck@1.25.2
	bytemuck_derive@1.12.1
	byteorder-lite@0.1.0
	bytes@1.12.1
	bzip2@0.6.1
	castaway@0.2.4
	cc@1.4.7
	cfg-if@1.0.5
	cfg_aliases@0.2.2
	chacha20@0.10.2
	clap@4.6.7
	clap_builder@4.6.7
	clap_complete@4.6.11
	clap_derive@4.6.7
	clap_lex@1.1.1
	clap_mangen@0.3.3
	color@0.3.3
	color_quant@1.1.0
	colorchoice@1.0.5
	compact_str@0.9.1
	confy@2.0.0
	const-oid@0.10.2
	convert_case@0.10.0
	cpufeatures@0.2.17
	cpufeatures@0.3.1
	crc32fast@1.5.2
	critical-section@1.2.0
	crossbeam-channel@0.5.17
	crossbeam-utils@0.8.23
	crossterm@0.28.1
	crossterm@0.29.0
	crossterm_winapi@0.9.1
	crunchy@0.2.4
	crypto-common@0.1.7
	crypto-common@0.2.2
	csscolorparser@0.6.2
	csv-core@0.1.13
	csv@1.4.0
	darling@0.24.1
	darling_core@0.24.1
	darling_macro@0.24.1
	data-url@0.3.2
	deltae@0.3.2
	deranged@0.5.8
	derive_more-impl@2.1.1
	derive_more@2.1.1
	digest@0.10.7
	digest@0.11.3
	document-features@0.2.12
	either@1.18.0
	equivalent@1.0.2
	errno@0.3.14
	etcetera@0.10.0
	euclid@0.22.14
	fallible-iterator@0.3.0
	fallible-streaming-iterator@0.1.9
	fancy-regex@0.11.0
	fancy-regex@0.16.2
	fastrand@2.5.0
	fax@0.2.7
	fdeflate@0.3.7
	fearless_simd@0.4.1
	filedescriptor@0.8.3
	filetime@0.2.29
	find-msvc-tools@0.1.13
	finl_unicode@1.5.0
	fixedbitset@0.4.2
	flate2@1.1.10
	float-cmp@0.9.0
	fnv@1.0.7
	foldhash@0.2.0
	font-types@0.11.3
	font-types@0.12.6
	funty@2.0.0
	futures-channel@0.3.34
	futures-core@0.3.34
	futures-executor@0.3.34
	futures-io@0.3.34
	futures-macro@0.3.34
	futures-sink@0.3.34
	futures-task@0.3.34
	futures-util@0.3.34
	futures@0.3.34
	generic-array@0.14.7
	getrandom@0.2.17
	getrandom@0.3.4
	getrandom@0.4.3
	gif@0.14.2
	git2@0.21.0
	glifo@0.1.1
	guillotiere@0.7.0
	half@2.7.1
	hashbrown@0.16.1
	hashbrown@0.17.1
	hashlink@0.12.2
	hayro-ccitt@0.3.0
	hayro-cmap@0.1.0
	hayro-interpret@0.7.0
	hayro-jbig2@0.3.0
	hayro-jpeg2000@0.3.5
	hayro-postscript@0.1.0
	hayro-syntax@0.7.2
	hayro@0.7.1
	heck@0.5.0
	hermit-abi@0.5.3
	hex@0.4.3
	home@0.5.12
	hybrid-array@0.4.15
	icy_sixel@0.5.1
	ident_case@1.0.1
	image-webp@0.2.4
	image@0.25.10
	imagesize@0.15.0
	indexmap@2.14.2
	indoc@2.0.7
	instability@0.3.14
	is_terminal_polyfill@1.70.2
	itertools@0.14.0
	itoa@1.0.18
	jobserver@0.1.35
	js-sys@0.3.105
	kasuari@0.4.12
	kurbo@0.13.1
	lab@0.11.0
	lazy_static@1.5.0
	libbz2-rs-sys@0.2.5
	libc@0.2.189
	libgit2-sys@0.18.8+1.9.7
	libm@0.2.16
	libmimalloc-sys@0.1.49
	libsqlite3-sys@0.38.2
	libz-sys@1.1.29
	line-clipping@0.3.8
	linebender_resource_handle@0.1.1
	linked-hash-map@0.5.6
	linux-raw-sys@0.12.1
	linux-raw-sys@0.4.15
	litrs@1.0.0
	lock_api@0.4.14
	log@0.4.34
	lru@0.18.5
	lzma-rust2@0.21.0
	mac_address@1.1.8
	matchers@0.2.0
	memchr@2.8.3
	memmem@0.1.1
	memoffset@0.9.1
	metrohash@1.0.7
	mimalloc@0.1.52
	minimal-lexical@0.2.1
	miniz_oxide@0.8.9
	miniz_oxide@0.9.1
	mio@1.2.3
	moxcms@0.8.1
	nix@0.29.0
	nom@7.1.3
	nu-ansi-term@0.50.3
	num-conv@0.2.2
	num-derive@0.4.2
	num-traits@0.2.19
	num_cpus@1.17.0
	num_threads@0.1.7
	numtoa@0.2.4
	once_cell@1.21.4
	once_cell_polyfill@1.70.2
	onig@6.5.3
	onig_sys@69.9.3
	ordered-float@4.6.0
	ordered-float@5.4.0
	outref@0.5.2
	palette@0.7.7
	palette_derive@0.7.7
	palette_math@0.7.7
	parking_lot@0.12.5
	parking_lot_core@0.9.12
	peniko@0.6.1
	pest@2.9.2
	pest_derive@2.9.2
	pest_generator@2.9.2
	pest_meta@2.9.2
	phf@0.11.3
	phf@0.13.1
	phf_codegen@0.11.3
	phf_generator@0.11.3
	phf_generator@0.13.1
	phf_macros@0.11.3
	phf_macros@0.13.1
	phf_shared@0.11.3
	phf_shared@0.13.1
	pic-scale@0.7.12
	pico-args@0.5.0
	pin-project-lite@0.2.17
	pkg-config@0.3.34
	plist@1.10.1
	png@0.18.1
	polycool@0.4.0
	portable-atomic@1.15.0
	powerfmt@0.2.0
	ppv-lite86@0.2.21
	proc-macro2@1.0.107
	pxfm@0.1.30
	quantette@0.6.0
	quick-error@2.0.1
	quick-xml@0.42.0
	quote@1.0.47
	r-efi@5.3.0
	r-efi@6.0.0
	radium@0.7.0
	rand@0.10.3
	rand@0.8.8
	rand_chacha@0.3.1
	rand_core@0.10.1
	rand_core@0.6.4
	rand_xoshiro@0.8.1
	ratatui-core@0.1.2
	ratatui-crossterm@0.1.2
	ratatui-image@11.1.0
	ratatui-macros@0.7.2
	ratatui-termina@0.1.0
	ratatui-termion@0.1.2
	ratatui-termwiz@0.1.2
	ratatui-widgets@0.3.2
	ratatui@0.30.2
	read-fonts@0.39.2
	read-fonts@0.45.0
	redox_syscall@0.5.18
	ref-cast-impl@1.0.27
	ref-cast@1.0.27
	regex-automata@0.4.18
	regex-syntax@0.8.11
	regex@1.13.1
	resvg@0.48.1
	rgb@0.8.53
	roff@1.1.1
	roxmltree@0.21.1
	rsqlite-vfs@0.1.1
	rusqlite@0.40.2
	rustc-hash@2.1.3
	rustc_version@0.4.1
	rustix@0.38.44
	rustix@1.1.5
	rustversion@1.0.23
	ryu@1.0.23
	safe_arch@1.2.0
	same-file@1.0.6
	scopeguard@1.2.0
	self_cell@1.3.0
	semver@1.0.28
	serde@1.0.229
	serde_core@1.0.229
	serde_derive@1.0.229
	serde_json@1.0.151
	serde_spanned@1.1.1
	sha2@0.10.9
	sha2@0.11.0
	sharded-slab@0.1.7
	shlex@2.0.1
	signal-hook-mio@0.2.5
	signal-hook-registry@1.4.8
	signal-hook@0.3.18
	simd-adler32@0.3.10
	simplecss@0.2.2
	siphasher@1.0.3
	skrifa@0.42.1
	skrifa@0.48.0
	slab@0.4.12
	smallvec@1.16.1
	socket2@0.6.5
	sqlite-wasm-rs@0.5.5
	stable_deref_trait@1.2.1
	static_assertions@1.1.0
	streaming-iterator@0.1.9
	strict-num@0.1.1
	strsim@0.11.1
	strum@0.28.0
	strum_macros@0.28.0
	svgtypes@0.16.1
	syn@1.0.109
	syn@2.0.119
	syn@3.0.6
	synstructure@0.14.0
	syntect@5.3.0
	tap@1.0.1
	tar@0.4.46
	tempfile@3.27.0
	termina@0.3.3
	terminfo@0.9.0
	termion@4.0.6
	termios@0.3.3
	termwiz@0.23.3
	thiserror-impl@1.0.69
	thiserror-impl@2.0.21
	thiserror@1.0.69
	thiserror@2.0.21
	thread_local@1.1.10
	tiff@0.11.3
	time-core@0.1.9
	time-macros@0.2.32
	time@0.3.55
	tiny-skia-path@0.12.0
	tiny-skia@0.12.0
	tokio-macros@2.7.2
	tokio@1.53.1
	toml@0.9.12+spec-1.1.0
	toml_datetime@0.7.5+spec-1.1.0
	toml_parser@1.1.3+spec-1.1.0
	toml_writer@1.1.2+spec-1.1.0
	tracing-attributes@0.1.31
	tracing-core@0.1.36
	tracing-error@0.2.1
	tracing-log@0.2.0
	tracing-subscriber@0.3.23
	tracing@0.1.44
	tree-sitter-bash@0.25.1
	tree-sitter-c-sharp@0.23.5
	tree-sitter-c@0.24.2
	tree-sitter-cpp@0.23.4
	tree-sitter-css@0.25.0
	tree-sitter-go@0.25.0
	tree-sitter-html@0.23.2
	tree-sitter-java@0.23.5
	tree-sitter-javascript@0.25.0
	tree-sitter-json@0.24.8
	tree-sitter-kotlin-ng@1.1.0
	tree-sitter-language@0.1.7
	tree-sitter-lua@0.5.0
	tree-sitter-php@0.24.2
	tree-sitter-python@0.25.0
	tree-sitter-r@1.3.0
	tree-sitter-ruby@0.23.1
	tree-sitter-rust@0.24.2
	tree-sitter-scala@0.24.1
	tree-sitter-swift@0.7.3
	tree-sitter-typescript@0.23.2
	tree-sitter-vim@0.4.0
	tree-sitter-xml@0.7.0
	tree-sitter-yaml@0.7.2
	tree-sitter@0.25.10
	two-face@0.5.2+bat-0.26.1
	typed-path@0.12.3
	typenum@1.20.1
	ucd-trie@0.1.7
	unicode-ident@1.0.26
	unicode-segmentation@1.13.3
	unicode-truncate@2.0.1
	unicode-width@0.2.2
	usvg@0.48.1
	utf8parse@0.2.2
	uuid@1.26.1
	valuable@0.1.1
	vcpkg@0.2.15
	vello_common@0.0.8
	vello_common@0.0.9
	vello_cpu@0.0.8
	version_check@0.9.5
	vsimd@0.8.0
	vtparse@0.6.2
	walkdir@2.5.0
	wasi@0.11.1+wasi-snapshot-preview1
	wasip2@1.0.4+wasi-0.2.12
	wasm-bindgen-macro-support@0.2.128
	wasm-bindgen-macro@0.2.128
	wasm-bindgen-shared@0.2.128
	wasm-bindgen@0.2.128
	weezl@0.1.12
	wezterm-bidi@0.2.3
	wezterm-blob-leases@0.1.1
	wezterm-color-types@0.3.0
	wezterm-dynamic-derive@0.1.1
	wezterm-dynamic@0.2.1
	wezterm-input-types@0.1.0
	wide@1.7.1
	winapi-i686-pc-windows-gnu@0.4.0
	winapi-util@0.1.11
	winapi-x86_64-pc-windows-gnu@0.4.0
	winapi@0.3.9
	windows-core@0.58.0
	windows-implement@0.58.0
	windows-interface@0.58.0
	windows-link@0.2.1
	windows-result@0.2.0
	windows-strings@0.1.0
	windows-sys@0.59.0
	windows-sys@0.61.2
	windows-targets@0.52.6
	windows@0.58.0
	windows_aarch64_gnullvm@0.52.6
	windows_aarch64_msvc@0.52.6
	windows_i686_gnu@0.52.6
	windows_i686_gnullvm@0.52.6
	windows_i686_msvc@0.52.6
	windows_x86_64_gnu@0.52.6
	windows_x86_64_gnullvm@0.52.6
	windows_x86_64_msvc@0.52.6
	winnow@0.7.15
	winnow@1.0.4
	wit-bindgen@0.57.1
	wuff@0.2.9
	wyz@0.5.1
	yaml-rust@0.4.5
	yoke-derive@0.8.4
	yoke@0.8.3
	zerocopy-derive@0.8.59
	zerocopy@0.8.59
	zerofrom-derive@0.1.8
	zerofrom@0.1.8
	zip@8.6.0
	zlib-rs@0.6.8
	zmij@1.0.23
	zune-core@0.5.3
	zune-jpeg@0.5.15
"

RUST_MIN_VER="1.88.0"

# bash-completion-r1 for `newbashcomp` in src_install; without it that call is an unbound command
# and the install phase dies.
inherit bash-completion-r1 cargo

DESCRIPTION="Fast, robust, accurate content-aware diffing"
HOMEPAGE="https://github.com/ivankovic/omnidiff"
SRC_URI="
	https://github.com/ivankovic/omnidiff/archive/refs/tags/v${PV}.tar.gz -> ${P}.tar.gz
	${CARGO_CRATE_URIS}
"

# The ebuild's own license is AGPL-3+; the trailing list covers the vendored crates, whose
# licenses cargo.eclass expects to be enumerated here. Regenerate with `pycargoebuild` if the
# dependency set changes - the list below was read off the crates in Cargo.lock and is the usual
# Rust-ecosystem spread.
LICENSE="AGPL-3+"
LICENSE+=" Apache-2.0 BSD BSD-2 BZIP2 ISC MIT MPL-2.0 Unicode-DFS-2016 Unlicense ZLIB"
SLOT="0"
KEYWORDS="~amd64 ~arm64"

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
