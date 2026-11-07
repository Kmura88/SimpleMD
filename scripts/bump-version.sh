#!/usr/bin/env bash
# 機能: バージョンを上げる
#
# 使い方:
#   scripts/bump-version.sh patch    # 0.0.1 → 0.0.2（不具合修正）
#   scripts/bump-version.sh minor    # 0.0.1 → 0.1.0（機能追加）
#   scripts/bump-version.sh major    # 0.1.0 → 1.0.0（大きな変更）
#   scripts/bump-version.sh 0.2.0    # 番号を直接指定
#
# 行うこと:
#   - Cargo.toml の version を書き換える（Cargo.lock も更新）
#   - CHANGELOG.md の「## [Unreleased]」の内容を「## [新バージョン] - 日付」の節に移す
# コミットと push は自分で行う。main に push すると GitHub Actions が自動でリリースを作る。
set -euo pipefail

cd "$(dirname "$0")/.."

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
if [[ ! "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "Cargo.toml の version ($current) が X.Y.Z 形式ではありません" >&2
  exit 1
fi
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

case "${1:-}" in
  patch) next="$major.$minor.$((patch + 1))" ;;
  minor) next="$major.$((minor + 1)).0" ;;
  major) next="$((major + 1)).0.0" ;;
  [0-9]*.[0-9]*.[0-9]*) next="$1" ;;
  *)
    echo "使い方: $0 patch|minor|major|X.Y.Z  （現在: $current）" >&2
    exit 1
    ;;
esac

if git rev-parse -q --verify "refs/tags/v$next" >/dev/null 2>&1; then
  echo "v$next は既にタグがあります" >&2
  exit 1
fi

# Cargo.toml（最初の version 行 = [package] の version だけを書き換える）
sed -i.bak "0,/^version = \".*\"/s//version = \"$next\"/" Cargo.toml && rm -f Cargo.toml.bak
# Cargo.lock の自分自身のバージョンも合わせる（ネットワークは使わない）
cargo update --workspace --offline >/dev/null 2>&1 || cargo metadata --format-version 1 >/dev/null

# CHANGELOG.md: [Unreleased] の下に新しいバージョンの見出しを入れる
today=$(date +%Y-%m-%d)
if grep -q '^## \[Unreleased\]' CHANGELOG.md; then
  awk -v v="$next" -v d="$today" '
    /^## \[Unreleased\]/ { print; print ""; print "## [" v "] - " d; next }
    { print }
  ' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md
else
  echo "注意: CHANGELOG.md に「## [Unreleased]」がないため、見出しを追加しませんでした" >&2
fi

echo "バージョンを $current → $next に変更しました。"
echo
echo "次の手順:"
echo "  1. CHANGELOG.md の [$next] の節に変更内容を書く（リリースノートになります）"
echo "  2. git add -A && git commit -m \"Release v$next\""
echo "  3. git push   → GitHub Actions が v$next のリリースを自動で作ります"
