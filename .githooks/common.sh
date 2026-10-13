# pre-commit / pre-push から読み込む共通処理

ALLOWED_EMAIL_SUFFIX="@users.noreply.github.com"

fail() {
  echo "" >&2
  echo "[snappi hook] $*" >&2
  exit 1
}

require_gitleaks() {
  command -v gitleaks >/dev/null 2>&1 ||
    fail "gitleaks が見つかりません。'winget install gitleaks' でインストールしてください。"
}

# 個人に固有の文字列を書いたローカル設定があれば、それを使う（リポジトリの設定を extend している）
gitleaks_config() {
  local_config="$(git rev-parse --git-dir)/gitleaks.local.toml"
  if [ -f "$local_config" ]; then
    echo "$local_config"
  else
    echo ".gitleaks.toml"
  fi
}

# メールアドレスが GitHub の noreply か
is_allowed_email() {
  case "$1" in
    *"$ALLOWED_EMAIL_SUFFIX") return 0 ;;
    noreply@github.com) return 0 ;; # GitHub 上でマージしたときのコミッター
    *) return 1 ;;
  esac
}
