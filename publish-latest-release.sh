#!/usr/bin/env bash

set -euo pipefail

readonly DEFAULT_REPOSITORY="golemfactory/yagna"
readonly DEFAULT_DESTINATION="s3://golem-releases/yagna"
readonly DEFAULT_REGION="eu-central-1"
readonly DEFAULT_CACHE_CONTROL="public, max-age=300"

usage() {
    cat <<'EOF'
Usage: publish-latest-release.sh <stable-release-tag> [--upload] [--output <path>]

Generate the CDN LATEST manifest from a stable GitHub release. The manifest is
uploaded to S3 only when --upload is provided.

Environment overrides:
  YAGNA_RELEASE_REPOSITORY     GitHub repository (default: golemfactory/yagna)
  YAGNA_RELEASE_DESTINATION    S3 prefix (default: s3://golem-releases/yagna)
  YAGNA_RELEASE_CACHE_CONTROL  Cache-Control value (default: public, max-age=300)
  AWS_REGION                   AWS region (default: eu-central-1)

Examples:
  ./publish-latest-release.sh v0.17.9
  ./publish-latest-release.sh v0.17.9 --upload
  mise exec -- ./publish-latest-release.sh v0.17.9 --upload
EOF
}

fail() {
    echo "error: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

[[ $# -gt 0 ]] || {
    usage >&2
    exit 2
}

tag=""
output="LATEST"
upload=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --upload)
            upload=true
            shift
            ;;
        --output)
            [[ $# -ge 2 ]] || fail "--output requires a path"
            output="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            fail "unknown option: $1"
            ;;
        *)
            [[ -z "$tag" ]] || fail "only one release tag can be specified"
            tag="$1"
            shift
            ;;
    esac
done

[[ -n "$tag" ]] || fail "a stable release tag is required"

require_command gh
require_command jq
if [[ "$upload" == true ]]; then
    require_command aws
fi

repository="${YAGNA_RELEASE_REPOSITORY:-$DEFAULT_REPOSITORY}"
destination="${YAGNA_RELEASE_DESTINATION:-$DEFAULT_DESTINATION}"
region="${AWS_REGION:-$DEFAULT_REGION}"
cache_control="${YAGNA_RELEASE_CACHE_CONTROL:-$DEFAULT_CACHE_CONTROL}"

[[ "$destination" == s3://*/* ]] || fail "invalid S3 destination: $destination"

if ! release_json="$(
    gh release view "$tag" \
        --repo "$repository" \
        --json tagName,name,publishedAt,isPrerelease,isDraft
)"; then
    fail "failed to fetch GitHub release $tag"
fi

output_dir="$(dirname "$output")"
[[ -d "$output_dir" ]] || fail "output directory does not exist: $output_dir"

tmp_file="$(mktemp "${output}.tmp.XXXXXX")"
trap 'rm -f "$tmp_file"' EXIT

if ! printf '%s\n' "$release_json" | jq -e '
    if .isDraft then
        error("LATEST cannot point to a draft release")
    elif .isPrerelease then
        error("LATEST cannot point to a prerelease")
    else
        {
            version: (.tagName | sub("^v"; "")),
            name: (if (.name // "") == "" then .tagName else .name end),
            released_at: .publishedAt
        }
    end
    | if
        (.version | test("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$"))
        and (.released_at | fromdateiso8601 | type == "number")
      then .
      else error("release contains invalid SemVer or publication timestamp")
      end
' > "$tmp_file"; then
    fail "GitHub release $tag is not a valid stable release"
fi

mv "$tmp_file" "$output"
trap - EXIT

echo "Generated $output from $repository release $tag:"
jq . "$output"

if [[ "$upload" != true ]]; then
    echo
    echo "Not uploaded. Re-run with --upload after all release artifacts are available in S3."
    exit 0
fi

destination="${destination%/}"
manifest_uri="$destination/LATEST"

aws s3 cp "$output" "$manifest_uri" \
    --region "$region" \
    --content-type "application/json" \
    --cache-control "$cache_control" \
    --no-progress

s3_path="${manifest_uri#s3://}"
bucket="${s3_path%%/*}"
key="${s3_path#*/}"

echo
echo "Uploaded $manifest_uri:"
aws s3api head-object \
    --bucket "$bucket" \
    --key "$key" \
    --region "$region" \
    --query '{ContentType:ContentType,CacheControl:CacheControl,Size:ContentLength,ETag:ETag}'
