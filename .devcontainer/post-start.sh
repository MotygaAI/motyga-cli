#!/usr/bin/env bash
set -euo pipefail

if [ "${MOTYGA_ENABLE_FIREWALL:-1}" != "1" ]; then
  echo "[devcontainer] Firewall mode: permissive (MOTYGA_ENABLE_FIREWALL=${MOTYGA_ENABLE_FIREWALL:-unset})."
  exit 0
fi

echo "[devcontainer] Firewall mode: strict"

domains_raw="${OPENAI_ALLOWED_DOMAINS:-api.openai.com}"
mapfile -t domains < <(printf '%s\n' "$domains_raw" | tr ', ' '\n\n' | sed '/^$/d' | sort -u)

if [ "${#domains[@]}" -eq 0 ]; then
  echo "[devcontainer] No allowed domains configured."
  exit 1
fi

tmp_file="$(mktemp)"
for domain in "${domains[@]}"; do
  if [[ ! "$domain" =~ ^[a-zA-Z0-9][a-zA-Z0-9.-]*\.[a-zA-Z]{2,}$ ]]; then
    echo "[devcontainer] Invalid domain in OPENAI_ALLOWED_DOMAINS: $domain"
    rm -f "$tmp_file"
    exit 1
  fi
  printf '%s\n' "$domain" >> "$tmp_file"
done

sudo install -d -m 0755 /etc/motyga
sudo cp "$tmp_file" /etc/motyga/allowed_domains.txt
sudo chown root:root /etc/motyga/allowed_domains.txt
sudo chmod 0444 /etc/motyga/allowed_domains.txt
rm -f "$tmp_file"

echo "[devcontainer] Applying firewall policy for domains: ${domains[*]}"
sudo --preserve-env=MOTYGA_INCLUDE_GITHUB_META_RANGES /usr/local/bin/init-firewall.sh
