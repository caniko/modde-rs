%global crate modde

Name:           modde
Version:        0.4.0
Release:        1%{?dist}
Summary:        Cross-platform game mod manager

License:        GPL-3.0-only
URL:            https://github.com/caniko/rs-modde
Source0:        %{url}/archive/v%{version}.tar.gz#/rs-modde-v%{version}.tar.gz
Source1:        vendor.tar.gz

BuildRequires:  rust >= 1.93
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  pkg-config
BuildRequires:  openssl-devel
BuildRequires:  dbus-devel
BuildRequires:  wayland-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  vulkan-loader-devel

%description
modde is a cross-platform game mod manager with CLI and GUI interfaces.
It supports Nexus Mods, Wabbajack modlists, FOMOD installers, and BAIN
packages for games like Skyrim, Fallout, Starfield, and Cyberpunk 2077.

%prep
%autosetup -n rs-modde -p1
tar xf %{SOURCE1}
mkdir -p .cargo
cat > .cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
cargo build --release --locked

%install
install -Dm755 target/release/modde %{buildroot}%{_bindir}/modde
install -Dm755 target/release/modde-ui %{buildroot}%{_bindir}/modde-ui
install -Dm644 dist/modde-ui.desktop %{buildroot}%{_datadir}/applications/com.tartanoglu.modde.desktop
install -Dm644 dist/com.tartanoglu.modde.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/com.tartanoglu.modde.png
install -Dm644 dist/assets/logo/logo.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/com.tartanoglu.modde.svg
install -Dm644 dist/com.tartanoglu.modde.metainfo.xml %{buildroot}%{_datadir}/metainfo/com.tartanoglu.modde.metainfo.xml

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/modde
%{_bindir}/modde-ui
%{_datadir}/applications/com.tartanoglu.modde.desktop
%{_datadir}/icons/hicolor/512x512/apps/com.tartanoglu.modde.png
%{_datadir}/icons/hicolor/scalable/apps/com.tartanoglu.modde.svg
%{_datadir}/metainfo/com.tartanoglu.modde.metainfo.xml
