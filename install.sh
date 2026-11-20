#!/bin/sh
# Installs chartos with cargo, and optionally the chartos agent skill.
#
#   curl -fsSL https://raw.githubusercontent.com/feryadialoi/chartos/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/feryadialoi/chartos/main/install.sh | sh -s -- --skill
#
# Run from a clone (./install.sh) to build the local checkout instead of the remote.
#
# Options:
#   --skill          Also install the agent skill to $SKILL_DIR (default ~/.claude/skills/chartos)
#   --skill-only     Install only the agent skill
#   --ref REF        Git branch or tag to install from (default main)
set -eu

REPO="https://github.com/feryadialoi/chartos"
RAW="https://raw.githubusercontent.com/feryadialoi/chartos"
REF="main"
SKILL=0
BINARY=1
SKILL_DIR="${SKILL_DIR:-$HOME/.claude/skills/chartos}"

while [ $# -gt 0 ]; do
  case "$1" in
    --skill) SKILL=1 ;;
    --skill-only) SKILL=1; BINARY=0 ;;
    --ref) shift; REF="${1:?--ref needs a value}" ;;
    -h|--help) sed -n '2,13p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '\033[1mchartos:\033[0m %s\n' "$*"; }

# A local checkout is used when the script runs from one.
LOCAL=""
case "$0" in
  */install.sh|install.sh)
    dir=$(cd "$(dirname "$0")" && pwd)
    [ -f "$dir/Cargo.toml" ] && grep -q '^name = "chartos"' "$dir/Cargo.toml" && LOCAL="$dir"
    ;;
esac

if [ "$BINARY" = 1 ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. Install Rust first: https://rustup.rs" >&2
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    exit 1
  fi
  if [ -n "$LOCAL" ]; then
    say "building from $LOCAL"
    cargo install --locked --path "$LOCAL"
  else
    say "building $REPO ($REF)"
    if git ls-remote --exit-code --tags "$REPO" "$REF" >/dev/null 2>&1; then
      cargo install --locked --git "$REPO" --tag "$REF"
    else
      cargo install --locked --git "$REPO" --branch "$REF"
    fi
  fi
  bin="${CARGO_HOME:-$HOME/.cargo}/bin"
  say "installed $(command -v chartos 2>/dev/null || echo "$bin/chartos")"
  case ":$PATH:" in *":$bin:"*) ;; *) say "add $bin to PATH" ;; esac
fi

if [ "$SKILL" = 1 ]; then
  mkdir -p "$SKILL_DIR"
  if [ -n "$LOCAL" ]; then
    cp "$LOCAL/skills/chartos/SKILL.md" "$SKILL_DIR/SKILL.md"
  else
    curl -fsSL "$RAW/$REF/skills/chartos/SKILL.md" -o "$SKILL_DIR/SKILL.md"
  fi
  say "skill installed to $SKILL_DIR"
fi
