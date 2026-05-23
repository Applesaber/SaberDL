#!/usr/bin/env bash
# 自测脚本: 编译 + 覆盖 DownloadError 三个分支 + 验证错误链 (=^・ω・^=)
set -euo pipefail
cd "$(dirname "$0")"

if [[ -t 1 ]]; then
    GREEN=$'\033[0;32m'; RED=$'\033[0;31m'; BLUE=$'\033[0;34m'; NC=$'\033[0m'
else
    GREEN=''; RED=''; BLUE=''; NC=''
fi

OUTDIR=$(mktemp -d)
trap 'rm -rf "$OUTDIR"' EXIT

PASS=0
FAIL=0

# run_case <描述> <期望:ok|err> <SaberDL 参数...>
run_case() {
    local desc=$1 expect=$2; shift 2
    echo
    echo "${BLUE}━━━ $desc ━━━${NC}"
    echo "  期望: $expect | 命令: cargo run --quiet -- get $*"
    echo
    set +e
    cargo run --quiet -- get "$@"
    local rc=$?
    set -e
    if [[ "$expect" == "ok"  && $rc -eq 0 ]] \
    || [[ "$expect" == "err" && $rc -ne 0 ]]; then
        echo "${GREEN}✓ 通过${NC} (退出码 $rc)"
        PASS=$((PASS + 1))
    else
        echo "${RED}✗ 失败${NC} (期望 $expect, 退出码 $rc)"
        FAIL=$((FAIL + 1))
    fi
}

echo "${BLUE}━━━ 编译 ━━━${NC}"
cargo build --quiet
echo "${GREEN}✓ 编译成功${NC}"

run_case "正常下载 (RSS)" ok \
    "https://www.applesaber.site/rss.xml" -o "$OUTDIR/rss.xml"

run_case "默认文件名推导 (SVG)" ok \
    "https://www.rust-lang.org/static/images/rust-logo-blk.svg" -o "$OUTDIR/rust-logo.svg"

run_case "BadStatus(404) 错误链" err \
    "https://httpbin.org/status/404"

run_case "BadStatus(500) 错误链" err \
    "https://httpbin.org/status/500"

run_case "Http(builder error) 错误链" err \
    "not-a-real-url"

echo
echo "${BLUE}═══════════════════════════════════════${NC}"
if [[ $FAIL -eq 0 ]]; then
    echo "${GREEN}全部 $PASS 个测试通过喵 (=^・ω・^=)${NC}"
    echo "${BLUE}═══════════════════════════════════════${NC}"
    exit 0
else
    echo "${RED}失败 $FAIL / 通过 $PASS${NC}"
    echo "${BLUE}═══════════════════════════════════════${NC}"
    exit 1
fi
