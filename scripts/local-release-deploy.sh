#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
publish_flag="${2:-}"
publish_version="${3:-}"
if [ -z "$version" ] || [ "$publish_flag" != "--publish" ] || [ "$publish_version" != "$version" ]; then
  echo "usage: local-release-deploy <version> --publish <version>" >&2
  echo "refusing to publish without an explicit matching confirmation" >&2
  exit 2
fi

repo="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo"
canix_root="${CANIX_ROOT:-/data/nvme0/can/canix}"

release_root="${MODDE_LOCAL_RELEASE_ROOT:-$repo/target/modde-release/$version}"
release_dir="${MODDE_LOCAL_RELEASE_DIR:-$release_root/release}"
work_dir="${MODDE_LOCAL_RELEASE_WORKDIR:-$release_root/work}"
srpm_dir="$work_dir/srpms"
release_env_file="$work_dir/release-env"
release_json_file="$work_dir/release.json"
mkdir -p "$release_dir" "$work_dir" "$srpm_dir"
export RELEASE_DIR="$release_dir"
export MODDE_LOCAL_RELEASE_WORKDIR="$work_dir"

missing=()
warnings=()
published=()
skipped=()
failed=()
repo_secret_names=""
repo_variable_names=""

. "$repo/scripts/release-local-env.sh"

record_published() {
  published+=("$1")
  printf 'published: %s\n' "$1"
}

record_skipped() {
  skipped+=("$1")
  printf 'skipped: %s\n' "$1" >&2
  if [ -f "$release_dir/artifacts.json" ]; then
    release_manifest_skip "$1" "$1"
  fi
}

run_publisher() {
  local name="$1"
  shift
  local skipped_before="${#skipped[@]}"
  if "$@"; then
    if [ "${#skipped[@]}" -eq "$skipped_before" ]; then
      record_published "$name"
    fi
  else
    failed+=("$name")
    return 1
  fi
}

is_prerelease() {
  [[ "$version" =~ - ]]
}

write_release_env() {
  cat > "$release_env_file" <<EOF
VERSION='$version'
IS_PRERELEASE='$(if is_prerelease; then printf true; else printf false; fi)'
EOF
}

require_tool_for_publish() {
  local destination="$1"
  local tool="$2"
  if command -v "$tool" >/dev/null 2>&1; then
    return 0
  fi
  record_skipped "$destination missing tool $tool"
  return 1
}

artifact_sha() {
  local artifact="$1"
  awk -v a="$artifact" '$2 == a { print $1 }' "$release_dir/SHA256SUMS.txt"
}

have_artifact() {
  local path="$1"
  [ -s "$path" ]
}

build_darwin_artifact() {
  local attr="$1" arch="$2" out_link="$3"
  local archive="$release_dir/modde-${version}-${arch}-darwin.tar.gz"
  if have_artifact "$archive"; then
    printf 'already-current: %s Darwin artifact exists\n' "$arch"
    return 0
  fi

  nix build ".#${attr}" --out-link "$out_link"
  mkdir -p "$release_dir/darwin-${arch}"
  copy_nix_binary "$out_link" modde "$release_dir/darwin-${arch}/modde"
  copy_nix_binary "$out_link" modde-ui "$release_dir/darwin-${arch}/modde-ui"
  tar czf "$archive" -C "$release_dir/darwin-${arch}" modde modde-ui
}

build_release_artifacts() {
  write_release_env
  export VERSION="$version"

  cargo deny check -W unmaintained advisories bans sources licenses

  mkdir -p "$srpm_dir" "$release_dir"
  shopt -s nullglob
  local existing_srpms=("$srpm_dir"/*.src.rpm)
  shopt -u nullglob
  if [ "${#existing_srpms[@]}" -eq 0 ]; then
    local source_tarball="$work_dir/rs-modde-v${version}.tar.gz"
    git archive --format=tar.gz --prefix=rs-modde/ -o "$source_tarball" HEAD
    tmp_vendor="$(mktemp -d "$work_dir/vendor.XXXXXX")"
    trap 'rm -rf "$tmp_vendor"' RETURN
    tar xf "$source_tarball" -C "$tmp_vendor"
    (cd "$tmp_vendor/rs-modde" && cargo vendor vendor > "$work_dir/cargo-vendor-config.toml")
    tar czf "$work_dir/vendor.tar.gz" -C "$tmp_vendor/rs-modde" vendor
    local spec_dir spec
    spec_dir="$(mktemp -d "$work_dir/spec.XXXXXX")"
    spec="${spec_dir}/dist/rpm/modde.spec"
    trap 'rm -rf "$spec_dir"; rm -rf "$tmp_vendor"' RETURN
    mkdir -p "$(dirname "$spec")"
    sed "0,/^Version:.*$/s//Version:        ${version}/" dist/rpm/modde.spec > "$spec"
    rpmbuild -bs "$spec" --define "_sourcedir $work_dir" --define "_srcrpmdir $srpm_dir"
  else
    printf 'already-current: SRPM artifact exists\n'
  fi

  if ! have_artifact "$release_dir/THIRD_PARTY_LICENSES.html"; then
    cargo about generate --config dist/licenses/about.toml --output-file "$release_dir/THIRD_PARTY_LICENSES.html" dist/licenses/about-template.hbs
  fi
  if ! have_artifact "$release_dir/modde-${version}.cdx.json"; then
    cargo sbom --output-format cyclone_dx_json_1_5 > "$release_dir/modde-${version}.cdx.json"
  fi
  if ! have_artifact "$release_dir/modde-${version}.spdx.json"; then
    cargo sbom --output-format spdx_json_2_3 > "$release_dir/modde-${version}.spdx.json"
  fi

  copy_nix_binary() {
    local result_dir="$1" binary="$2" destination="$3"
    if [ -f "${result_dir}/bin/.${binary}-wrapped" ]; then
      cp "${result_dir}/bin/.${binary}-wrapped" "$destination"
    else
      cp "${result_dir}/bin/${binary}" "$destination"
    fi
  }

  if ! have_artifact "$release_dir/modde-${version}-x86_64-linux.tar.gz"; then
    nix build .#modde --out-link "$work_dir/linux-result"
    mkdir -p "$release_dir/linux-x86_64"
    copy_nix_binary "$work_dir/linux-result" modde "$release_dir/linux-x86_64/modde"
    copy_nix_binary "$work_dir/linux-result" modde-ui "$release_dir/linux-x86_64/modde-ui"
    tar czf "$release_dir/modde-${version}-x86_64-linux.tar.gz" -C "$release_dir/linux-x86_64" modde modde-ui
    cp "$release_dir/linux-x86_64/modde" "$release_dir/modde-${version}-x86_64-linux"
    cp "$release_dir/linux-x86_64/modde-ui" "$release_dir/modde-ui-${version}-x86_64-linux"
  else
    printf 'already-current: x86_64 Linux artifacts exist\n'
  fi

  if [ "${MODDE_LOCAL_DEPLOY_SKIP_AARCH64:-0}" = "1" ]; then
    record_skipped "aarch64 Linux artifact build skipped by MODDE_LOCAL_DEPLOY_SKIP_AARCH64=1"
  elif ! have_artifact "$release_dir/modde-${version}-aarch64-linux.tar.gz"; then
    nix build .#modde-aarch64-linux --out-link "$work_dir/aarch64-linux-result"
    mkdir -p "$release_dir/linux-aarch64"
    copy_nix_binary "$work_dir/aarch64-linux-result" modde "$release_dir/linux-aarch64/modde"
    copy_nix_binary "$work_dir/aarch64-linux-result" modde-ui "$release_dir/linux-aarch64/modde-ui"
    tar czf "$release_dir/modde-${version}-aarch64-linux.tar.gz" -C "$release_dir/linux-aarch64" modde modde-ui
    cp "$release_dir/linux-aarch64/modde" "$release_dir/modde-${version}-aarch64-linux"
    cp "$release_dir/linux-aarch64/modde-ui" "$release_dir/modde-ui-${version}-aarch64-linux"
  else
    printf 'already-current: aarch64 Linux artifacts exist\n'
  fi

  if ! have_artifact "$release_dir/modde-${version}-x86_64-windows.zip"; then
    nix build .#modde-windows --out-link "$work_dir/windows-result"
    mkdir -p "$release_dir/windows-x86_64"
    cp "$work_dir/windows-result/bin/modde.exe" "$work_dir/windows-result/bin/modde-ui.exe" "$release_dir/windows-x86_64/"
    if [ -f "$work_dir/windows-result/bin/libmcfgthread-2.dll" ]; then
      cp "$work_dir/windows-result/bin/libmcfgthread-2.dll" "$release_dir/windows-x86_64/"
    fi
  else
    printf 'already-current: Windows archive exists\n'
  fi

  if ! have_artifact "$release_dir/modde-ui-${version}-x86_64.AppImage"; then
    nix build .#appimage-ui --out-link "$work_dir/appimage-ui-result"
    cp "$work_dir/appimage-ui-result" "$release_dir/modde-ui-${version}-x86_64.AppImage"
  fi
  if ! have_artifact "$release_dir/modde-${version}-x86_64.AppImage"; then
    nix build .#appimage-cli --out-link "$work_dir/appimage-cli-result"
    cp "$work_dir/appimage-cli-result" "$release_dir/modde-${version}-x86_64.AppImage"
  fi

  if ! have_artifact "$release_dir/rs-modde-${version}.tar.gz"; then
    git archive --format=tar.gz --prefix=rs-modde/ -o "$release_dir/rs-modde-${version}.tar.gz" HEAD
  fi
  if ! have_artifact "$release_dir/com.tartanoglu.modde.json"; then
    source_sha256="$(sha256sum "$release_dir/rs-modde-${version}.tar.gz" | awk '{print $1}')"
    nix build .#flatpak-manifest --out-link "$work_dir/flatpak-result"
    cp "$work_dir/flatpak-result" "$release_dir/com.tartanoglu.modde.json"
    sed -i "s/@SOURCE_TARBALL_SHA256@/${source_sha256}/" "$release_dir/com.tartanoglu.modde.json"
  fi
  if ! have_artifact "$release_dir/cargo-sources.json"; then
    nix run .#flatpak-cargo-generator -- Cargo.lock -o "$release_dir/cargo-sources.json"
  fi
  cp "$srpm_dir"/*.src.rpm "$release_dir"/ 2>/dev/null || true

  if ! have_artifact "$release_dir/modde_${version}_amd64.deb"; then
    if ! bash scripts/build-deb.sh "$version" "$release_dir"; then
      record_skipped "deb artifacts unavailable; APT publish will be skipped unless existing .deb artifacts are present"
    fi
  else
    printf 'already-current: Debian artifacts exist\n'
  fi

  if [ ! -d "$release_dir/windows-x86_64" ]; then
    record_skipped "Windows artifacts absent; Chocolatey/Scoop/Winget will be skipped"
  elif [ -n "${WINDOWS_SIGNING_PFX:-}" ] && [ -n "${WINDOWS_SIGNING_PASS:-}" ] && [ ! -s "$release_dir/modde-${version}-x86_64-windows.zip" ]; then
    pfx_file="$(mktemp "$work_dir/windows-signing-pfx.XXXXXX")"
    pass_file="$(mktemp "$work_dir/windows-signing-pass.XXXXXX")"
    trap 'rm -f "$pfx_file" "$pass_file"' RETURN
    printf '%s' "$WINDOWS_SIGNING_PFX" | base64 --decode > "$pfx_file"
    printf '%s' "$WINDOWS_SIGNING_PASS" > "$pass_file"
    for exe in "$release_dir/windows-x86_64/modde.exe" "$release_dir/windows-x86_64/modde-ui.exe"; do
      osslsigncode sign -pkcs12 "$pfx_file" -readpass "$pass_file" -h sha256 \
        -n 'modde' -i 'https://modde.tartanoglu.com' -ts 'http://timestamp.digicert.com' \
        -in "$exe" -out "${exe}.signed"
      osslsigncode verify -in "${exe}.signed"
      mv "${exe}.signed" "$exe"
    done
  elif [ ! -s "$release_dir/modde-${version}-x86_64-windows.zip" ]; then
    record_skipped "windows authenticode signing credentials absent; publishing unsigned Windows artifacts"
  fi
  if [ -d "$release_dir/windows-x86_64" ] && [ ! -s "$release_dir/modde-${version}-x86_64-windows.zip" ]; then
    for exe in "$release_dir/windows-x86_64/modde.exe" "$release_dir/windows-x86_64/modde-ui.exe"; do
      test -s "$exe"
    done
    tar czf "$release_dir/modde-${version}-x86_64-windows.tar.gz" -C "$release_dir/windows-x86_64" modde.exe modde-ui.exe
    zip_out="$release_dir/modde-${version}-x86_64-windows.zip"
    (cd "$release_dir/windows-x86_64" && zip -q "$zip_out" modde.exe modde-ui.exe)
    cp "$release_dir/windows-x86_64/modde.exe" "$release_dir/modde-${version}-x86_64-windows.exe"
    cp "$release_dir/windows-x86_64/modde-ui.exe" "$release_dir/modde-ui-${version}-x86_64-windows.exe"
  fi

  if [ "${MODDE_LOCAL_DEPLOY_SKIP_DARWIN:-0}" = "1" ]; then
    record_skipped "Darwin artifact build skipped by MODDE_LOCAL_DEPLOY_SKIP_DARWIN=1; Homebrew will be skipped"
  else
    build_darwin_artifact modde-darwin-aarch64 aarch64 "$work_dir/darwin-arm-result"
  fi

  (
    cd "$release_dir"
    shopt -s nullglob
    : > SHA256SUMS.txt
    for file in *.tar.gz *.zip *.AppImage *.deb *.src.rpm *.cdx.json *.spdx.json; do
      [ "$file" != SHA256SUMS.txt ] || continue
      sha256sum "$file" >> SHA256SUMS.txt
    done
    test -s SHA256SUMS.txt
  )

  test -s keys/minisign.pub
  test -n "${MINISIGN_SECRET_KEY:-}"
  test -n "${MINISIGN_PASSWORD:-}"
  umask 077
  minisign_key="$(mktemp "$work_dir/minisign.XXXXXX")"
  trap 'rm -f "$minisign_key"' RETURN
  printf '%s' "$MINISIGN_SECRET_KEY" > "$minisign_key"
  printf '%s\n' "$MINISIGN_PASSWORD" | minisign -S -s "$minisign_key" -m "$release_dir/SHA256SUMS.txt" -x "$release_dir/SHA256SUMS.txt.minisig"
  minisign -V -m "$release_dir/SHA256SUMS.txt" -x "$release_dir/SHA256SUMS.txt.minisig" -p keys/minisign.pub

  if [ -n "${COSIGN_PRIVATE_KEY:-}" ]; then
    cosign_key="$(mktemp "$work_dir/cosign.XXXXXX")"
    trap 'rm -f "$cosign_key"' RETURN
    printf '%s' "$COSIGN_PRIVATE_KEY" > "$cosign_key"
    shopt -s nullglob
    for file in "$release_dir"/*.tar.gz "$release_dir"/*.zip "$release_dir"/*.AppImage "$release_dir"/*.deb "$release_dir"/*.src.rpm "$release_dir"/*.exe; do
      cosign sign-blob --yes --key "$cosign_key" --bundle "${file}.cosign.bundle" "$file"
    done
  else
    record_skipped "cosign unavailable locally; minisign checksums are authoritative for local deploy"
  fi
  release_manifest_collect_release_files "rs-modde-local-build"

  if [ -z "${FLATHUB_TOKEN:-}" ]; then
    MODDE_FLATPAK_MANIFEST_ONLY=1 run_release_smoke
  else
    run_release_smoke
  fi
}

run_release_smoke() {
  local smoke_failed=0
  for script in scripts/smoke/smoke-*.sh; do
    case "$(basename "$script")" in
      smoke-srpm.sh)
        record_skipped "SRPM local Fedora rebuild skipped during deploy; COPR remote build validates the SRPM"
        continue
        ;;
    esac
    bash "$script" "$version" "$release_dir" || smoke_failed=1
  done
  return "$smoke_failed"
}

extract_release_notes() {
  local notes_file="$1"
  awk -v version="$version" '
    $0 ~ "^## \\[" version "\\] - [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$" { found = 1; print; next }
    found && /^## \[/ { exit }
    found && /^\[[^]]+\]: / { exit }
    found { print }
    END { if (!found) exit 1 }
  ' CHANGELOG.md > "$notes_file" || {
    echo "CHANGELOG.md missing section for $version" >&2
    return 1
  }
}

publish_codeberg_release() {
  require_local_secret_for_publish "Codeberg release" CODEBERG_TOKEN \
    'Codeberg fj auth store or canix runtime secret can_codeberg_token' \
    'fj -H codeberg.org auth status || jq '\''.hosts["codeberg.org"] | {type, name, token_present: (.token != null and .token != "")}'\'' "$HOME/.local/share/forgejo-cli/keys.json"' || return 0

  local api="${CODEBERG_API:-https://codeberg.org/api/v1}"
  local codeberg_repo="${CODEBERG_REPO:-caniko/rs-modde}"
  local payload status release_id asset_id file name release_notes_file
  release_notes_file="$work_dir/release-notes.md"
  extract_release_notes "$release_notes_file"
  payload="$(jq -n --arg tag "$version" --arg name "$version" --arg branch "trunk" --argjson prerelease "$(if is_prerelease; then printf true; else printf false; fi)" --rawfile body "$release_notes_file" '{tag_name: $tag, target_commitish: $branch, name: $name, body: $body, draft: false, prerelease: $prerelease}')"
  status="$(curl -sS -o "$release_json_file" -w '%{http_code}' -H "Authorization: token ${CODEBERG_TOKEN}" -H 'Content-Type: application/json' -d "$payload" "${api}/repos/${codeberg_repo}/releases")"
  if [ "$status" = "409" ]; then
    curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/tags/${version}" > "$release_json_file"
  elif [ "$status" -lt 200 ] || [ "$status" -ge 300 ]; then
    cat "$release_json_file"
    return 1
  fi
  release_id="$(jq -r '.id' "$release_json_file")"
  test "$release_id" != "null"
  shopt -s nullglob
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    name="$(basename "$file")"
    asset_id="$(curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets" | jq -r --arg name "$name" '.[] | select(.name == $name) | .id' | head -n 1)"
    if [ -n "$asset_id" ]; then
      curl -sS --fail -X DELETE -H "Authorization: token ${CODEBERG_TOKEN}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets/${asset_id}"
    fi
    curl -sS --fail -H "Authorization: token ${CODEBERG_TOKEN}" -H 'Content-Type: application/octet-stream' --data-binary "@${file}" "${api}/repos/${codeberg_repo}/releases/${release_id}/assets?name=${name}" > /dev/null
  done < <(printf '%s\n' "$release_dir"/* | LC_ALL=C sort -u)
}

publish_homebrew() {
  is_prerelease && { record_skipped "Homebrew prerelease"; return 0; }
  require_local_secret_for_publish "Homebrew" HOMEBREW_TAP_TOKEN \
    'export HOMEBREW_TAP_TOKEN for codeberg.org/caniko/homebrew-modde or add a canix runtime secret for it' \
    'git -c credential.helper='\''!f() { echo username=x-access-token; echo "password=$HOMEBREW_TAP_TOKEN"; }; f'\'' ls-remote https://codeberg.org/caniko/homebrew-modde.git HEAD' || return 0

  local artifact
  for artifact in \
    "$release_dir/modde-${version}-aarch64-darwin.tar.gz" \
    "$release_dir/modde-${version}-aarch64-linux.tar.gz" \
    "$release_dir/modde-${version}-x86_64-linux.tar.gz"; do
    if ! have_artifact "$artifact"; then
      record_skipped "Homebrew missing required artifact ${artifact}"
      return 0
    fi
  done

  local credential_helper='!f() { echo username=x-access-token; echo "password=$HOMEBREW_TAP_TOKEN"; }; f'
  local homebrew_tap="$work_dir/homebrew-tap"
  rm -rf "$homebrew_tap"
  git -c credential.helper="$credential_helper" clone "${HOMEBREW_TAP_URL:-https://codeberg.org/caniko/homebrew-modde.git}" "$homebrew_tap"
  (
    cd "$homebrew_tap"
    git config credential.helper "$credential_helper"
    git config user.email 'release-bot@localhost'
    git config user.name 'release bot'
    git remote set-head origin -a
    default_branch="$(git symbolic-ref --short refs/remotes/origin/HEAD | sed 's|^origin/||')"
    git checkout "$default_branch"
  )

  nix run '.#rs-harbor' -- brew bump \
    --name modde \
    --version "$version" \
    --description 'Cross-platform game mod manager' \
    --homepage 'https://modde.tartanoglu.com' \
    --license GPL-3.0-only \
    --archive "darwin_arm=https://github.com/caniko/rs-modde/releases/download/${version}/modde-${version}-aarch64-darwin.tar.gz,$release_dir/modde-${version}-aarch64-darwin.tar.gz" \
    --archive "linux_arm=https://github.com/caniko/rs-modde/releases/download/${version}/modde-${version}-aarch64-linux.tar.gz,$release_dir/modde-${version}-aarch64-linux.tar.gz" \
    --archive "linux_intel=https://github.com/caniko/rs-modde/releases/download/${version}/modde-${version}-x86_64-linux.tar.gz,$release_dir/modde-${version}-x86_64-linux.tar.gz" \
    --binary modde \
    --binary modde-ui \
    --tap "$homebrew_tap"

  (
    cd "$homebrew_tap"
    if [ -z "$(git status --porcelain -- Formula/modde.rb)" ]; then
      printf 'already-current: Homebrew tap\n'
      exit 0
    fi
    git add Formula/modde.rb
    git commit -m "modde ${version}"
    git push origin "HEAD:${default_branch}"
  )
}

publish_apt() {
  is_prerelease && { record_skipped "APT prerelease"; return 0; }
  require_local_secret_for_publish "APT" APT_REPO_GPG_KEY \
    "$canix_root/age/secrets/root/modules/repos/apt/modde_apt_repo_gpg_key.age" \
    'export APT_REPO_GPG_KEY=...; gpg --show-keys --with-fingerprint <(printf %s "$APT_REPO_GPG_KEY")' || return 0
  require_local_secret_for_publish "APT" APT_REPO_SSH_KEY \
    "$canix_root/age/secrets/root/modules/repos/apt/modde_apt_repo_ssh_key.age" \
    'ssh -i <(printf %s "$APT_REPO_SSH_KEY") -T git@codeberg.org' || return 0
  shopt -s nullglob
  local debs=("$release_dir"/*.deb)
  shopt -u nullglob
  [ "${#debs[@]}" -gt 0 ] || { record_skipped "APT missing .deb artifacts"; return 0; }
  VERSION="$version" RELEASE_DIR="$release_dir" APT_REPO_BRANCH="${APT_REPO_BRANCH:-pages}" ./scripts/publish-apt.sh
}

publish_aur() {
  is_prerelease && { record_skipped "AUR prerelease"; return 0; }
  require_local_secret_for_publish "AUR" AUR_SSH_KEY \
    'canix runtime secret can_aur_ssh_key or exported AUR_SSH_KEY' \
    'ssh -i <(printf %s "$AUR_SSH_KEY") -T aur@aur.archlinux.org' || return 0
  require_tool_for_publish "AUR" makepkg || return 0
  local makepkg_conf source_sha bin_sha
  makepkg_conf="$(dirname "$(dirname "$(command -v makepkg)")")/etc/makepkg.conf"
  source_sha="$(artifact_sha "rs-modde-${version}.tar.gz")"
  bin_sha="$(artifact_sha "modde-${version}-x86_64-linux.tar.gz")"
  test -n "$source_sha"
  test -n "$bin_sha"
  install -d -m 700 "$HOME/.ssh"
  local aur_key="$HOME/.ssh/aur"
  printf '%s\n' "$AUR_SSH_KEY" > "$aur_key"
  chmod 600 "$aur_key"
  ssh-keyscan aur.archlinux.org >> "$HOME/.ssh/known_hosts" 2>/dev/null
  chmod 600 "$HOME/.ssh/known_hosts"
  export GIT_SSH_COMMAND="ssh -i $aur_key -o IdentitiesOnly=yes"
  publish_pkg() {
    local pkg="$1" repo_url="ssh://aur@aur.archlinux.org/${pkg}.git"
    local aur_checkout="$work_dir/aur-${pkg}"
    rm -rf "$aur_checkout"
    if ! git clone "$repo_url" "$aur_checkout"; then
      mkdir "$aur_checkout"
      git -C "$aur_checkout" init
      git -C "$aur_checkout" remote add origin "$repo_url"
    fi
    git -C "$aur_checkout" checkout -B master
    cp "dist/aur/${pkg}/PKGBUILD" "$aur_checkout/PKGBUILD"
    case "$pkg" in
      modde)
        sed -i -e "s/^pkgver=.*/pkgver=${version}/" -e 's/^pkgrel=.*/pkgrel=1/' "$aur_checkout/PKGBUILD"
        awk -v sha="$source_sha" '/^sha256sums=/{print "sha256sums=(\047" sha "\047)"; next} {print}' "$aur_checkout/PKGBUILD" > "$aur_checkout/PKGBUILD.new"
        mv "$aur_checkout/PKGBUILD.new" "$aur_checkout/PKGBUILD"
        ;;
      modde-bin)
        sed -i -e "s/^pkgver=.*/pkgver=${version}/" -e 's/^pkgrel=.*/pkgrel=1/' "$aur_checkout/PKGBUILD"
        awk -v b="$bin_sha" -v s="$source_sha" '/^sha256sums=/{print "sha256sums=(\047" b "\047"; print "            \047" s "\047)"; skip=1; next} skip && /^[[:space:]]*\047/{next} {skip=0; print}' "$aur_checkout/PKGBUILD" > "$aur_checkout/PKGBUILD.new"
        mv "$aur_checkout/PKGBUILD.new" "$aur_checkout/PKGBUILD"
        ;;
      modde-git) ;;
    esac
    (cd "$aur_checkout" && makepkg --config "$makepkg_conf" --printsrcinfo > .SRCINFO)
    git -C "$aur_checkout" add PKGBUILD .SRCINFO
    if git -C "$aur_checkout" diff --cached --quiet; then
      printf 'already-current: AUR %s\n' "$pkg"
      return 0
    fi
    git -C "$aur_checkout" -c user.email='release-bot@localhost' -c user.name='release bot' commit -m "$version"
    git -C "$aur_checkout" push origin HEAD:master
  }
  for pkg in modde modde-bin modde-git; do publish_pkg "$pkg"; done
}

publish_copr() {
  require_local_secret_for_publish "COPR" COPR_LOGIN 'canix runtime secret can_coppr_login' 'test -n "$COPR_LOGIN"' || return 0
  require_local_secret_for_publish "COPR" COPR_USERNAME "$canix_root/age/secrets/root/modules/repos/coppr/username" 'test -n "$COPR_USERNAME"' || return 0
  require_local_secret_for_publish "COPR" COPR_TOKEN 'canix runtime secret can_coppr_token' 'test -n "$COPR_TOKEN"' || return 0
  shopt -s nullglob
  local srpms=("$srpm_dir"/*.src.rpm)
  shopt -u nullglob
  [ "${#srpms[@]}" -gt 0 ] || { record_skipped "COPR missing SRPM"; return 0; }
  local copr_config copr_name project chroot_args=()
  copr_config="$(mktemp -d "$work_dir/copr-config.XXXXXX")"
  cat > "$copr_config/copr" <<EOF
[copr-cli]
login = ${COPR_LOGIN}
username = ${COPR_USERNAME}
token = ${COPR_TOKEN}
copr_url = https://copr.fedorainfracloud.org
EOF
  chmod 600 "$copr_config/copr"
  copr_name="${COPR_PROJECT_NAME:-rs-modde}"
  is_prerelease && copr_name="${COPR_PROJECT_NAME:-rs-modde-testing}"
  project="${COPR_USERNAME}/${copr_name}"

  if ! XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- get "$project" >/dev/null 2>&1; then
    local chroot
    for chroot in ${COPR_CHROOTS:-fedora-43-x86_64 fedora-42-x86_64 epel-10-x86_64}; do
      chroot_args+=(--chroot "$chroot")
    done
    XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- create "$copr_name" \
      "${chroot_args[@]}" \
      --description 'modde release builds' \
      --instructions 'Install with: sudo dnf copr enable caniko/rs-modde && sudo dnf install modde'
  fi

  XDG_CONFIG_HOME="$copr_config" nix run .#copr-cli -- build --nowait "$project" "$srpm_dir"/*.src.rpm
  rm -rf "$copr_config"
}

publish_windows_packagers() {
  is_prerelease && { record_skipped "Windows packagers prerelease"; return 0; }
  local zip_name="modde-${version}-x86_64-windows.zip"
  test -s "$release_dir/${zip_name}" || { record_skipped "Windows packagers missing Windows zip"; return 0; }

  local simit_bin
  simit_bin="${SIMIT_BIN:-simit}"
  if ! "$simit_bin" dist windows publish --help | grep -q -- '--force-resubmit'; then
    printf 'Windows package publishing requires a simit binary with `dist windows publish`; set SIMIT_BIN to the fixed simit binary or update the rs-modde simit flake input.\n' >&2
    return 1
  fi

  local args=(
    dist windows publish
    --version "$version"
    --archive "x64=$release_dir/${zip_name}"
    --work-dir "$work_dir/windows-packagers"
  )

  if require_local_secret_for_publish "Chocolatey" CHOCOLATEY_API_KEY 'canix runtime secret can_choco_api_key' 'test -n "$CHOCOLATEY_API_KEY"'; then
    args+=(--chocolatey --choco-push-source "${CHOCO_PUSH_SOURCE:-https://push.chocolatey.org/}")
    [ "${CHOCOLATEY_FORCE_RESUBMIT:-0}" = "1" ] && args+=(--force-resubmit)
  else
    record_skipped "Chocolatey missing API key"
  fi

  if require_local_secret_for_publish "Scoop" SCOOP_BUCKET_TOKEN 'export SCOOP_BUCKET_TOKEN for github.com/caniko/scoop-modde' 'test -n "$SCOOP_BUCKET_TOKEN"'; then
    args+=(--scoop --scoop-bucket-url "${SCOOP_BUCKET_URL:-https://github.com/caniko/scoop-modde.git}" --scoop-bucket-token-env SCOOP_BUCKET_TOKEN)
  else
    record_skipped "Scoop missing bucket token"
  fi

  if require_local_secret_for_publish "Winget" WINGET_PAT 'export WINGET_PAT for github.com/microsoft/winget-pkgs' 'test -n "$WINGET_PAT"'; then
    args+=(--winget)
  else
    record_skipped "Winget missing GitHub token"
  fi

  if [[ ! " ${args[*]} " =~ " --chocolatey " ]] && [[ ! " ${args[*]} " =~ " --scoop " ]] && [[ ! " ${args[*]} " =~ " --winget " ]]; then
    record_skipped "Windows packagers missing publish credentials"
    return 0
  fi

  "$simit_bin" "${args[@]}"
}

publish_flathub() {
  is_prerelease && { record_skipped "Flathub prerelease"; return 0; }
  require_local_secret_for_publish "Flathub" FLATHUB_TOKEN 'export FLATHUB_TOKEN for github.com/flathub/com.tartanoglu.modde' 'test -n "$FLATHUB_TOKEN"' || return 0
  test -s "$release_dir/com.tartanoglu.modde.json" || { record_skipped "Flathub missing manifest"; return 0; }
  test -s "$release_dir/cargo-sources.json" || { record_skipped "Flathub missing cargo sources"; return 0; }
  local credential_helper='!f() { echo username=x-access-token; echo "password=$FLATHUB_TOKEN"; }; f'
  local flathub_repo="$work_dir/flathub-repo"
  rm -rf "$flathub_repo"
  git -c credential.helper="$credential_helper" clone https://github.com/flathub/com.tartanoglu.modde.git "$flathub_repo"
  (
    cd "$flathub_repo"
    git config credential.helper "$credential_helper"
    git config user.email 'release-bot@localhost'
    git config user.name 'release bot'
    git checkout -B "release/${version}"
    cp "$release_dir/com.tartanoglu.modde.json" "$release_dir/cargo-sources.json" .
    if [ -z "$(git status --porcelain -- com.tartanoglu.modde.json cargo-sources.json)" ]; then
      printf 'already-current: Flathub manifest\n'
      exit 0
    fi
    git add com.tartanoglu.modde.json cargo-sources.json
    git commit -m "$version"
    git push --force-with-lease origin "release/${version}"
  )
  local payload status
  payload="$(jq -n --arg title "$version" --arg head "release/${version}" --arg base "master" --arg body "Update to ${version}." '{title: $title, head: $head, base: $base, body: $body}')"
  status="$(curl -sS -o pr.json -w '%{http_code}' -H "Authorization: Bearer ${FLATHUB_TOKEN}" -H 'Accept: application/vnd.github+json' -d "$payload" https://api.github.com/repos/flathub/com.tartanoglu.modde/pulls)"
  if [ "$status" -eq 422 ]; then
    printf 'already-current: Flathub PR exists or rejected\n'
    return 0
  fi
  if [ "$status" -lt 200 ] || [ "$status" -ge 300 ]; then
    cat pr.json
    return 1
  fi
}

print_summary() {
  printf '\nlocal release deploy summary for %s\n' "$version"
  printf 'published:\n'
  if [ "${#published[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${published[@]}"; fi
  printf 'skipped:\n'
  if [ "${#skipped[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${skipped[@]}"; fi
  printf 'failed:\n'
  if [ "${#failed[@]}" -eq 0 ]; then printf '  - none\n'; else printf '  - %s\n' "${failed[@]}"; fi
  printf 'policy:\n'
  printf '  - crates.io publish remains CI-only\n'
  printf '  - macOS artifacts are ad-hoc signed and not notarized\n'
}

if [ "${SIMIT_LOCAL_RELEASE_CHECK_DONE:-0}" != "1" ]; then
  nix run "$repo#local-check-release" -- "$version"
fi
load_canix_release_inputs
build_release_artifacts

run_publisher "Codeberg release" publish_codeberg_release
run_publisher "Homebrew tap" publish_homebrew
run_publisher "APT repository" publish_apt
run_publisher "AUR packages" publish_aur
run_publisher "COPR" publish_copr
run_publisher "Windows packagers" publish_windows_packagers
run_publisher "Flathub" publish_flathub
record_skipped "crates.io publish remains CI-only"

print_summary
test "${#failed[@]}" -eq 0
