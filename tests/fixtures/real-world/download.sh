#!/bin/bash
# Download real-world CSV fixtures used by perf smoke tests.
#
# These datasets are not checked in (too large / redistribution unclear).
# Run this once before running tests that exercise real-world fixtures —
# otherwise those tests will skip with a note.
#
# Idempotent: re-running skips files that already pass validation.

set -e
cd "$(dirname "$0")"

# Columns: filename | url | mode | expected
#   mode=sha256: expected is a sha256 hex digest, enforced strictly.
#   mode=sanity: expected is "<min_bytes>:<header_prefix>" — size + header check,
#                used for live-data sources that drift (e.g. Google Sheets).
FIXTURES=(
  "chronic_disease.csv|https://data.cdc.gov/api/views/hksd-2xuw/rows.csv?accessType=DOWNLOAD|sha256|cb73f4a1b6ae9be9f94a58f71c3e2cf3d0628dfd94120d64be54d21cd2e0ffdf"
  "customers-10000.csv|https://drive.google.com/uc?id=1x2IdSNcHGLmot9i1h90gwMJr5lULC2QV&export=download|sha256|4ec8a3053cd1ad252e4eaa75d0c4759031b4a654975587eb8eee5a7fdfa3146d"
  "uncleaned_ds_jobs.csv|https://raw.githubusercontent.com/eyowhite/Messy-dataset/main/Uncleaned_DS_jobs.csv|sha256|c291b1040857bec5a34a1fc11792d31b1c8a717799a21588749e9181fd5be547"
  "salary_survey.csv|https://docs.google.com/spreadsheets/d/1IPS5dBSGtwYVbjsfbaMCYIWnOuRmJcbequohNxCyGVw/export?format=csv|sanity|4000000:Timestamp,How old are you?"
)

validate() {
  local file="$1" mode="$2" expected="$3"
  case "$mode" in
    sha256)
      local actual
      actual=$(shasum -a 256 "$file" | awk '{print $1}')
      [[ "$actual" == "$expected" ]]
      ;;
    sanity)
      local min_bytes header_prefix size header
      min_bytes="${expected%%:*}"
      header_prefix="${expected#*:}"
      size=$(wc -c < "$file" | tr -d ' ')
      header=$(head -c "${#header_prefix}" "$file")
      [[ "$size" -ge "$min_bytes" && "$header" == "$header_prefix" ]]
      ;;
    *)
      echo "unknown mode: $mode" >&2
      return 1
      ;;
  esac
}

for entry in "${FIXTURES[@]}"; do
  IFS='|' read -r name url mode expected <<< "$entry"

  if [[ -f "$name" ]] && validate "$name" "$mode" "$expected"; then
    echo "OK   $name (already present, valid)"
    continue
  fi

  echo "GET  $name"
  curl -fsSL -H "User-Agent: Mozilla/5.0" "$url" -o "$name"

  if ! validate "$name" "$mode" "$expected"; then
    echo "FAIL $name — validation failed ($mode)"
    if [[ "$mode" == sha256 ]]; then
      echo "  expected sha256: $expected"
      echo "  actual sha256:   $(shasum -a 256 "$name" | awk '{print $1}')"
    fi
    exit 1
  fi
  echo "OK   $name"
done

echo
echo "All fixtures ready. Run tests with: cargo test"
